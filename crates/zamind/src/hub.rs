//! The event hub (ADR-0006): sequence numbers assigned once at ingest,
//! bounded per-subscriber queues, per-server log rings bounded by servers —
//! never by clients. A slow client receives a `missed` marker and catches
//! up through the file-backed APIs; it can never stall the daemon.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use zamin_core::logparse::{self, PlayerLogEvent};
use zamin_protocol::streams::{
    CoreEvent, LogLine, MetricsSample, StreamCursor, StreamKind, StreamNotification, StreamPayload,
};

const SUBSCRIBER_QUEUE: usize = 1024;
const EVENT_RING: usize = 256;
const LOG_RING: usize = 5000;
/// 1 Hz samples, ten minutes of history per server (ADR-0006: preallocated
/// rings bounded by servers, history served by `metrics.range`).
const METRICS_RING: usize = 600;

/// One subscription's inbound channel. The session forwards from it to the
/// wire; a full channel means the wire is slow, and the hub degrades the
/// subscriber instead of blocking.
pub struct Subscription {
    pub id: String,
    pub receiver: mpsc::Receiver<StreamNotification>,
}

#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("cursor is older than what replay can serve")]
    CursorInvalid,
    #[error("cursor references an unknown stream")]
    CursorUnknown,
}

struct SubscriberState {
    stream: StreamKind,
    server_id: Option<String>,
    sender: mpsc::Sender<StreamNotification>,
    /// Highest sequence delivered to this subscriber.
    cursor: u64,
    /// Notifications that could not be enqueued while the subscriber was
    /// slow. Reported through a single `Missed` marker when delivery
    /// resumes; the cursor never advances for undelivered sequences.
    pending_missed: u64,
    /// Metrics latest-wins slot (ADR-0006): when the queue is full, the
    /// newest undelivered sample waits here and older ones are dropped —
    /// a stale sample has no value. Flushed on the next publish once a
    /// slot opens; never turned into a `Missed` marker.
    pending_metrics: Option<StreamNotification>,
}

#[derive(Default)]
struct ServerRings {
    events: VecDeque<(u64, CoreEvent)>,
    logs: VecDeque<LogLine>,
    metrics: VecDeque<(u64, MetricsSample)>,
}

pub struct Hub {
    seq: AtomicU64,
    inner: Mutex<Inner>,
}

struct Inner {
    subscribers: HashMap<String, SubscriberState>,
    rings: BTreeMap<String, ServerRings>,
    log_total: BTreeMap<String, u64>,
    /// Live player rosters, derived from join/leave log lines (ADR-0011's
    /// phase carried this refinement: the ping sample caps at 12 names,
    /// the log does not). One set per server; emptied when the server's
    /// process exits.
    rosters: BTreeMap<String, std::collections::BTreeSet<String>>,
}

/// Cloneable handle into the hub for publishers (actors) and sessions.
#[derive(Clone)]
pub struct HubHandle {
    hub: Arc<Hub>,
}

impl HubHandle {
    pub fn new() -> HubHandle {
        HubHandle {
            hub: Arc::new(Hub {
                seq: AtomicU64::new(1),
                inner: Mutex::new(Inner {
                    subscribers: HashMap::new(),
                    rings: BTreeMap::new(),
                    log_total: BTreeMap::new(),
                    rosters: BTreeMap::new(),
                }),
            }),
        }
    }
}

impl HubHandle {
    /// Current ingest sequence — the cursor a fresh logs subscription
    /// resumes from.
    pub fn current_seq(&self) -> u64 {
        self.hub.seq.load(Ordering::SeqCst)
    }

    /// Ingest a domain event: ring it, assign a sequence, fan out.
    pub fn publish_event(&self, server_id: Option<String>, event: CoreEvent) {
        let seq = self.hub.seq.fetch_add(1, Ordering::SeqCst);
        let mut inner = self
            .hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rings = inner
            .rings
            .entry(server_id.clone().unwrap_or_default())
            .or_default();
        rings.events.push_back((seq, event.clone()));
        while rings.events.len() > EVENT_RING {
            rings.events.pop_front();
        }
        let notification = StreamNotification {
            stream: StreamKind::Events,
            server_id: server_id.clone(),
            seq,
            payload: StreamPayload::Event { event },
        };
        fan_out(&mut inner, &notification);
    }

    /// Ingest a batch of parsed log lines (called by the actor's batcher).
    pub fn publish_logs(&self, server_id: &str, lines: Vec<LogLine>) {
        if lines.is_empty() {
            return;
        }
        let mut inner = self
            .hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut batch = Vec::with_capacity(lines.len());
        let mut first_seq = None;
        {
            let Inner {
                rings,
                log_total,
                rosters,
                ..
            } = &mut *inner;
            let rings = rings.entry(server_id.to_owned()).or_default();
            let roster = rosters.entry(server_id.to_owned()).or_default();
            for line in lines {
                *log_total.entry(server_id.to_owned()).or_default() += 1;
                let seq = self.hub.seq.fetch_add(1, Ordering::SeqCst);
                first_seq.get_or_insert(seq);
                batch.push(line.clone());
                rings.logs.push_back(line);
                while rings.logs.len() > LOG_RING {
                    rings.logs.pop_front();
                }
            }
            // The roster reads the same lines the ring stores — one parse
            // pass per batch, applied after the ring so nothing delays the
            // fan-out path.
            for line in &batch {
                match logparse::player_event(&line.line) {
                    Some(PlayerLogEvent::Joined(name)) => {
                        roster.insert(name);
                    }
                    Some(PlayerLogEvent::Left(name)) => {
                        roster.remove(&name);
                    }
                    None => {}
                }
            }
        }
        if let Some(seq) = first_seq {
            let notification = StreamNotification {
                stream: StreamKind::Logs,
                server_id: Some(server_id.to_owned()),
                seq,
                payload: StreamPayload::Logs { batch },
            };
            fan_out(&mut inner, &notification);
        }
    }

    /// Ingest one metrics sample (the actor's 1 Hz sampler). Ring it for
    /// history, fan out with latest-wins semantics (ADR-0006).
    pub fn publish_metrics(&self, server_id: &str, sample: MetricsSample) {
        let seq = self.hub.seq.fetch_add(1, Ordering::SeqCst);
        let mut inner = self
            .hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rings = inner.rings.entry(server_id.to_owned()).or_default();
        rings.metrics.push_back((seq, sample));
        while rings.metrics.len() > METRICS_RING {
            rings.metrics.pop_front();
        }
        let notification = StreamNotification {
            stream: StreamKind::Metrics,
            server_id: Some(server_id.to_owned()),
            seq,
            payload: StreamPayload::Metrics { sample },
        };
        fan_out(&mut inner, &notification);
    }

    /// Subscribe. `cursor` replays what the ring can serve; anything older
    /// answers `CursorInvalid` and the client re-snapshots (ADR-0006).
    /// Fresh logs subscriptions receive the current ring as their first
    /// batch and resume from the ingest cursor.
    ///
    /// Replay and registration happen under one lock hold: a publisher
    /// cannot ingest between them, so there is no gap in which events are
    /// silently lost.
    pub fn subscribe(
        &self,
        stream: StreamKind,
        server_id: Option<String>,
        cursor: Option<StreamCursor>,
    ) -> Result<Subscription, HubError> {
        let (sender, receiver) = mpsc::channel(SUBSCRIBER_QUEUE);
        let id = format!("sub-{}", uuid::Uuid::now_v7());

        let seq_cursor = match (&cursor, stream) {
            (Some(StreamCursor::Events { seq }), StreamKind::Events) => Some(*seq),
            (Some(StreamCursor::Logs { .. }) | None, StreamKind::Logs) => Some(self.current_seq()),
            (None, StreamKind::Events) => None,
            (None, StreamKind::Metrics) => None,
            (Some(_), _) => return Err(HubError::CursorUnknown),
        };

        let mut inner = self
            .hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Replay for events streams.
        let mut last_replayed = seq_cursor;
        if stream == StreamKind::Events {
            if let Some(from) = seq_cursor {
                let rings = inner
                    .rings
                    .get(server_id.as_deref().unwrap_or_default())
                    .ok_or(HubError::CursorInvalid)?;
                let oldest = rings.events.front().map(|(seq, _)| *seq);
                if oldest.is_some_and(|oldest| oldest > from) {
                    return Err(HubError::CursorInvalid);
                }
                for (seq, event) in rings.events.iter().filter(|(s, _)| *s > from) {
                    let _ = sender.try_send(StreamNotification {
                        stream,
                        server_id: server_id.clone(),
                        seq: *seq,
                        payload: StreamPayload::Event {
                            event: event.clone(),
                        },
                    });
                    last_replayed = Some(*seq);
                }
            }
        }

        // Fresh logs subscriptions: deliver the ring as the opening batch.
        if stream == StreamKind::Logs && cursor.is_none() {
            if let Some(rings) = inner.rings.get(server_id.as_deref().unwrap_or_default()) {
                if !rings.logs.is_empty() {
                    let batch: Vec<LogLine> =
                        rings.logs.iter().rev().take(500).rev().cloned().collect();
                    let _ = sender.try_send(StreamNotification {
                        stream,
                        server_id: server_id.clone(),
                        seq: self.current_seq(),
                        payload: StreamPayload::Logs { batch },
                    });
                }
            }
        }

        // Fresh metrics subscriptions: the latest ring sample first, so a
        // client shows last-known numbers immediately instead of waiting a
        // full sample interval.
        if stream == StreamKind::Metrics && cursor.is_none() {
            if let Some(rings) = inner.rings.get(server_id.as_deref().unwrap_or_default()) {
                if let Some(&(seq, sample)) = rings.metrics.back() {
                    let _ = sender.try_send(StreamNotification {
                        stream,
                        server_id: server_id.clone(),
                        seq,
                        payload: StreamPayload::Metrics { sample },
                    });
                    last_replayed = Some(seq);
                }
            }
        }

        inner.subscribers.insert(
            id.clone(),
            SubscriberState {
                stream,
                server_id,
                sender,
                cursor: last_replayed.unwrap_or(0),
                pending_missed: 0,
                pending_metrics: None,
            },
        );

        Ok(Subscription { id, receiver })
    }

    pub fn unsubscribe(&self, id: &str) {
        self.hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .subscribers
            .remove(id);
    }

    /// The stored log ring (for the initial terminal view and tests).
    pub fn log_ring(&self, server_id: &str) -> Vec<LogLine> {
        let inner = self
            .hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .rings
            .get(server_id)
            .map(|r| r.logs.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The stored metrics ring, oldest first (serves `metrics.range` and
    /// tests). Bounded at [`METRICS_RING`] samples per server.
    pub fn metrics_ring(&self, server_id: &str) -> Vec<MetricsSample> {
        let inner = self
            .hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .rings
            .get(server_id)
            .map(|r| r.metrics.iter().map(|(_, s)| *s).collect())
            .unwrap_or_default()
    }

    /// The live roster size, `None` when the server has not logged anything
    /// yet — the honest "not measured" for the metrics `players` field.
    /// An existing entry with zero players is measured: `Some(0)`.
    pub fn roster_len(&self, server_id: &str) -> Option<usize> {
        let inner = self
            .hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.rosters.get(server_id).map(|set| set.len())
    }

    /// The live roster: players whose joins have not been followed by a
    /// leave, sorted for stable display. Empty for a server that never
    /// logged a join — including adopted servers (their history predates
    /// this daemon's log pumps).
    pub fn roster(&self, server_id: &str) -> Vec<String> {
        let inner = self
            .hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .rosters
            .get(server_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The server's process is gone; nobody is on it. Called by the log
    /// pump when the stdout pipe closes.
    pub fn clear_roster(&self, server_id: &str) {
        self.hub
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .rosters
            .remove(server_id);
    }
}

fn fan_out(inner: &mut Inner, notification: &StreamNotification) {
    let stale: Vec<String> = inner
        .subscribers
        .iter()
        .filter(|(_, s)| subscriber_matches(s, notification))
        .map(|(id, _)| id.clone())
        .collect();
    for id in stale {
        let Some(state) = inner.subscribers.get_mut(&id) else {
            continue;
        };
        // Metrics: latest-wins per subscriber (ADR-0006). A slow subscriber
        // keeps the newest undelivered sample and drops the stale ones; the
        // pending sample is flushed here as soon as a slot opens. History
        // comes from `metrics.range`, never from a Missed marker — a stale
        // sample has no value to catch up to.
        if matches!(notification.payload, StreamPayload::Metrics { .. }) {
            if let Some(pending) = state.pending_metrics.take() {
                let pending_seq = pending.seq;
                if state.sender.try_send(pending).is_ok() {
                    state.cursor = pending_seq;
                } else {
                    // Still no room: the fresh sample replaces the stale one.
                    state.pending_metrics = Some(notification.clone());
                    continue;
                }
            }
            if state.sender.try_send(notification.clone()).is_ok() {
                state.cursor = notification.seq;
            } else {
                state.pending_metrics = Some(notification.clone());
            }
            continue;
        }
        // The cursor only advances for sequences that actually entered the
        // queue; a reconnect resuming from the cursor must never skip past
        // events that were dropped (ADR-0006: no silent loss).
        if state.pending_missed > 0 {
            // The subscriber was slow earlier: lead with the accumulated
            // marker once a slot is free. If even the marker cannot fit,
            // keep accumulating and do not advance.
            let missed = StreamNotification {
                stream: notification.stream,
                server_id: notification.server_id.clone(),
                seq: notification.seq,
                payload: StreamPayload::Missed {
                    missed: state.pending_missed,
                },
            };
            match state.sender.try_send(missed) {
                Ok(()) => {
                    state.pending_missed = 0;
                    if state.sender.try_send(notification.clone()).is_ok() {
                        state.cursor = notification.seq;
                    } else {
                        state.pending_missed += 1;
                    }
                }
                Err(_) => state.pending_missed += 1,
            }
        } else if state.sender.try_send(notification.clone()).is_ok() {
            state.cursor = notification.seq;
        } else {
            state.pending_missed += 1;
        }
    }
}

fn subscriber_matches(state: &SubscriberState, notification: &StreamNotification) -> bool {
    if state.stream != notification.stream {
        return false;
    }
    match &state.server_id {
        Some(want) => notification.server_id.as_deref() == Some(want),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use zamin_protocol::jobs::{Job, JobKind, JobState};
    use zamin_protocol::server::ServerState;

    fn sample(ts_ms: i64) -> MetricsSample {
        MetricsSample {
            ts_ms,
            cpu_percent: Some(ts_ms as f64),
            rss_bytes: Some(1024),
            players: Some(0),
            tps: None,
            uptime_ms: Some(ts_ms),
        }
    }

    fn state_event(seq_hint: u64) -> CoreEvent {
        let _ = seq_hint;
        CoreEvent::ServerStateChanged {
            server_id: "s".to_owned(),
            from: ServerState::Starting,
            to: ServerState::Running,
            reason: None,
            exit_code: None,
            error: None,
            crash: None,
        }
    }

    fn job_event() -> CoreEvent {
        CoreEvent::JobStarted {
            job: Job {
                job_id: uuid::Uuid::now_v7(),
                kind: JobKind::BackupCreate,
                server_id: Some("s".to_owned()),
                state: JobState::Running,
                progress: None,
                error: None,
                created_at_ms: 0,
                started_at_ms: None,
                ended_at_ms: None,
            },
        }
    }

    /// Everything currently queued, without awaiting.
    fn drain(receiver: &mut mpsc::Receiver<StreamNotification>) -> Vec<StreamNotification> {
        let mut out = Vec::new();
        while let Ok(notification) = receiver.try_recv() {
            out.push(notification);
        }
        out
    }

    #[test]
    fn fresh_metrics_subscription_receives_the_latest_ring_sample() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            let hub = HubHandle::new();
            assert!(hub.metrics_ring("s").is_empty());

            hub.publish_metrics("s", sample(1));
            hub.publish_metrics("s", sample(2));
            hub.publish_metrics("s", sample(3));

            let mut sub = hub
                .subscribe(StreamKind::Metrics, Some("s".to_owned()), None)
                .unwrap();
            let first = sub.receiver.recv().await.unwrap();
            match first.payload {
                StreamPayload::Metrics { sample } => assert_eq!(sample.ts_ms, 3),
                other => panic!("expected a metrics sample, got {other:?}"),
            }
        });
    }

    #[test]
    fn metrics_ring_is_bounded_and_keeps_the_newest() {
        let hub = HubHandle::new();
        for ts in 1..=METRICS_RING as i64 + 50 {
            hub.publish_metrics("s", sample(ts));
        }
        let ring = hub.metrics_ring("s");
        assert_eq!(ring.len(), METRICS_RING);
        assert_eq!(ring.first().unwrap().ts_ms, 51); // 650 - 600 + 1
        assert_eq!(ring.last().unwrap().ts_ms, 650);
    }

    #[tokio::test]
    async fn slow_metrics_subscriber_coalesces_latest_wins() {
        let hub = HubHandle::new();
        let mut sub = hub
            .subscribe(StreamKind::Metrics, None, None)
            .expect("global metrics sub");

        // Overflow the 1024-slot queue; never drain. The subscriber keeps
        // only the newest undelivered sample (ADR-0006 latest-wins).
        for ts in 1..=1200i64 {
            hub.publish_metrics("s", sample(ts));
        }
        let queued = drain(&mut sub.receiver);
        assert_eq!(queued.len(), SUBSCRIBER_QUEUE);
        for (index, notification) in queued.iter().enumerate() {
            assert!(
                matches!(notification.payload, StreamPayload::Metrics { .. }),
                "no Missed markers on the metrics stream, got {:?}",
                notification.payload
            );
            assert_eq!(notification.seq, (index + 1) as u64);
        }

        // One more publish: the pending newest (1200) flushes ahead of the
        // fresh one (1201). Everything between 1025 and 1199 was dropped
        // silently — a stale sample has no value.
        hub.publish_metrics("s", sample(1201));
        let flushed = drain(&mut sub.receiver);
        let seqs: Vec<u64> = flushed.iter().map(|n| n.seq).collect();
        assert_eq!(seqs, vec![1200, 1201]);
    }

    #[tokio::test]
    async fn slow_events_subscriber_still_gets_the_missed_marker() {
        let hub = HubHandle::new();
        let mut sub = hub
            .subscribe(StreamKind::Events, None, None)
            .expect("global events sub");

        for _ in 0..1200u64 {
            hub.publish_event(Some("s".to_owned()), state_event(0));
        }
        let queued = drain(&mut sub.receiver);
        assert_eq!(queued.len(), SUBSCRIBER_QUEUE);

        hub.publish_event(Some("s".to_owned()), job_event());
        let after = drain(&mut sub.receiver);
        let missed: Vec<u64> = after
            .iter()
            .filter_map(|n| match n.payload {
                StreamPayload::Missed { missed } => Some(missed),
                _ => None,
            })
            .collect();
        assert_eq!(missed, vec![1200 - SUBSCRIBER_QUEUE as u64]);
    }
}
