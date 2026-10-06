//! The event hub (ADR-0006): sequence numbers assigned once at ingest,
//! bounded per-subscriber queues, per-server log rings bounded by servers —
//! never by clients. A slow client receives a `missed` marker and catches
//! up through the file-backed APIs; it can never stall the daemon.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use zamin_protocol::streams::{
    CoreEvent, LogLine, StreamCursor, StreamKind, StreamNotification, StreamPayload,
};

const SUBSCRIBER_QUEUE: usize = 1024;
const EVENT_RING: usize = 256;
const LOG_RING: usize = 5000;

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
}

#[derive(Default)]
struct ServerRings {
    events: VecDeque<(u64, CoreEvent)>,
    logs: VecDeque<LogLine>,
}

pub struct Hub {
    seq: AtomicU64,
    inner: Mutex<Inner>,
}

struct Inner {
    subscribers: HashMap<String, SubscriberState>,
    rings: BTreeMap<String, ServerRings>,
    log_total: BTreeMap<String, u64>,
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
                rings, log_total, ..
            } = &mut *inner;
            let rings = rings.entry(server_id.to_owned()).or_default();
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

        inner.subscribers.insert(
            id.clone(),
            SubscriberState {
                stream,
                server_id,
                sender,
                cursor: last_replayed.unwrap_or(0),
                pending_missed: 0,
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
