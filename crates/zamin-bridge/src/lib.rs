//! `zamin-bridge`: the panel host's forwarder, testable without a webview.
//!
//! ADR-0003 makes the Tauri host a *bridge only*: connection management,
//! forwarding, and the ~50 ms coalescing of daemon → webview frames
//! (PERFORMANCE-BUDGETS: "batched ~50 ms via the bridge channel; never one
//! message per line"). All of that is plain tokio, so it lives here where
//! the workspace can test it; the Tauri crate is a thin shell over it.

use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use zamin_ipc::{ConnectionReadHalf, ConnectionWriteHalf};

pub mod remote;

/// Coalescing window for daemon → webview frames.
pub const BATCH_WINDOW: Duration = Duration::from_millis(50);
/// Hard cap on frames per channel message, so a flood still chunks.
pub const BATCH_MAX_FRAMES: usize = 256;

/// Coalesces frames into batches. Pure state machine — time is injected so
/// tests drive it deterministically.
///
/// The first buffered frame opens the window; the batch flushes when the
/// window closes, when `max_frames` fills, or when the caller flushes.
#[derive(Debug)]
pub struct FrameBatcher {
    window: Duration,
    max_frames: usize,
    buffered: Vec<String>,
    window_opened_at: Option<Instant>,
}

impl FrameBatcher {
    pub fn new(window: Duration, max_frames: usize) -> FrameBatcher {
        FrameBatcher {
            window,
            max_frames,
            buffered: Vec::new(),
            window_opened_at: None,
        }
    }

    /// Buffer one frame. Returns a ready batch when `max_frames` filled.
    pub fn push(&mut self, frame: String, now: Instant) -> Option<Vec<String>> {
        if self.buffered.is_empty() {
            self.window_opened_at = Some(now);
        }
        self.buffered.push(frame);
        if self.buffered.len() >= self.max_frames {
            return Some(self.take());
        }
        None
    }

    /// The batch if the window has closed by `now`.
    pub fn flush_due(&mut self, now: Instant) -> Option<Vec<String>> {
        let opened = self.window_opened_at?;
        if now.duration_since(opened) >= self.window {
            return Some(self.take());
        }
        None
    }

    /// Deadline the pump should wake at to flush on time.
    pub fn next_flush(&self) -> Option<Instant> {
        self.window_opened_at.map(|opened| opened + self.window)
    }

    pub fn is_empty(&self) -> bool {
        self.buffered.is_empty()
    }

    fn take(&mut self) -> Vec<String> {
        self.window_opened_at = None;
        std::mem::take(&mut self.buffered)
    }
}

/// Spawn the read side of a daemon connection: frames from the wire arrive
/// coalesced on `incoming` as JSON arrays of frame strings. When the daemon
/// drops the wire (clean close or error), `on_down` fires exactly once and
/// the task ends.
///
/// The batch channel is bounded like every other queue in this daemon: a
/// stalled webview consumer degrades with dropped batches rather than
/// stalling the daemon (ADR-0006); the client re-snapshots after gaps.
pub fn spawn_read(
    mut read: ConnectionReadHalf,
    incoming: mpsc::Sender<Vec<String>>,
    on_down: impl FnOnce() + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut batcher = FrameBatcher::new(BATCH_WINDOW, BATCH_MAX_FRAMES);
        // Wake at half the window so a batch never waits more than the
        // window past its first frame.
        let mut ticker = tokio::time::interval(BATCH_WINDOW / 2);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // the first tick fires immediately

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Some(batch) = batcher.flush_due(Instant::now()) {
                        if incoming.send(batch).await.is_err() {
                            break;
                        }
                    }
                }
                frame = read.recv() => match frame {
                    Ok(Some(frame)) => {
                        let frame = String::from_utf8_lossy(&frame).into_owned();
                        if let Some(batch) = batcher.push(frame, Instant::now()) {
                            if incoming.send(batch).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(None) | Err(_) => break,
                },
            }
        }

        // Flush whatever the window still holds before signaling down.
        if !batcher.is_empty() {
            let _ = incoming.send(batcher.take()).await;
        }
        tracing::debug!("daemon read side closed");
        on_down();
    })
}

/// Write one frame to the daemon, framed per the wire format.
pub async fn send_frame(
    write: &mut ConnectionWriteHalf,
    frame: &str,
) -> Result<(), zamin_ipc::IpcError> {
    let payload = bytes::Bytes::from(frame.to_owned());
    write.send(payload).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn coalesces_frames_within_the_window() {
        let start = Instant::now();
        let mut batcher = FrameBatcher::new(BATCH_WINDOW, 256);

        assert!(batcher.push("a".into(), start).is_none());
        assert!(batcher
            .push("b".into(), start + Duration::from_millis(10))
            .is_none());
        assert!(batcher
            .flush_due(start + Duration::from_millis(30))
            .is_none());

        let batch = batcher
            .flush_due(start + BATCH_WINDOW)
            .expect("window closed");
        assert_eq!(batch, vec!["a", "b"]);
        assert!(batcher.is_empty());
    }

    #[test]
    fn flushes_immediately_when_max_frames_fills() {
        let start = Instant::now();
        let mut batcher = FrameBatcher::new(BATCH_WINDOW, 2);
        assert!(batcher.push("a".into(), start).is_none());
        let batch = batcher.push("b".into(), start).expect("cap reached");
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn window_reopens_after_each_batch() {
        let start = Instant::now();
        let mut batcher = FrameBatcher::new(BATCH_WINDOW, 256);
        assert!(batcher.push("a".into(), start).is_none());
        assert!(batcher.flush_due(start + BATCH_WINDOW).is_some());
        // Next frame opens a fresh window.
        assert!(batcher.push("b".into(), start + BATCH_WINDOW).is_none());
        assert!(batcher.flush_due(start + BATCH_WINDOW).is_none());
        assert!(batcher.flush_due(start + 2 * BATCH_WINDOW).is_some());
    }

    #[test]
    fn next_flush_tracks_the_open_window() {
        let start = Instant::now();
        let mut batcher = FrameBatcher::new(BATCH_WINDOW, 256);
        assert_eq!(batcher.next_flush(), None);
        assert!(batcher.push("a".into(), start).is_none());
        assert_eq!(batcher.next_flush(), Some(start + BATCH_WINDOW));
    }
}
