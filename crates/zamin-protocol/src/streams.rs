//! Streams: subscriptions, cursors, and notification payloads (protocol
//! spec §6, ADR-0006). Every notification carries a monotonic `seq` assigned
//! once at ingest, per server per stream.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::jobs::{Job, JobOutcome, JobProgress};
use crate::server::ServerState;
use crate::ProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamKind {
    Events,
    Logs,
    Metrics,
}

/// Position in a stream. Shape depends on the stream type:
/// - `events`: a sequence number into the bounded replay ring.
/// - `logs`: file identity + offset into the server's own log files;
///   rotation changes the file identity and invalidates the cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StreamCursor {
    Events { seq: u64 },
    Logs { file: String, offset: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeParams {
    pub stream: StreamKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<StreamCursor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsSnapshot {
    /// Full current truth to reconcile against: every registered server
    /// with its identity and lifecycle state (ADR-0006).
    pub servers: Vec<crate::server::ServerSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeResult {
    pub subscription_id: String,
    /// Cursor to resume from on reconnect (where the stream supports it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<StreamCursor>,
    /// Events stream: the snapshot to reconcile against before live events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<EventsSnapshot>,
    /// A requested cursor was older than what replay can serve; the client
    /// must re-snapshot instead of looping (ADR-0006).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cursor_invalid: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeParams {
    pub subscription_id: String,
}

/// Parsed log line. `ts_ms` is the ingestion time; JVM log timestamps are
/// local-time strings without offset and are preserved raw in the log file
/// APIs (protocol spec §9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub ts_ms: i64,
    pub level: LogLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    pub line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSample {
    pub ts_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rss_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<u32>,
    /// Only ever set when actually measured (ADR for metrics: no fake TPS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_ms: Option<i64>,
}

/// Domain events carried on the `events` stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CoreEvent {
    ServerStateChanged {
        server_id: String,
        from: ServerState,
        to: ServerState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ProtocolError>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        crash: Option<crate::server::CrashClassification>,
    },
    JobStarted {
        job: Job,
    },
    JobProgress {
        job_id: Uuid,
        progress: JobProgress,
    },
    JobCompleted {
        job_id: Uuid,
        outcome: JobOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ProtocolError>,
    },
}

/// Payload of a stream notification. Tagged, so unknown payload kinds are
/// rejected at the type level rather than silently misread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StreamPayload {
    Logs {
        batch: Vec<LogLine>,
    },
    Event {
        event: CoreEvent,
    },
    Metrics {
        sample: MetricsSample,
    },
    /// The subscriber fell behind: N notifications were dropped at the
    /// bounded queue. Catch up through the file-backed APIs (ADR-0006).
    Missed {
        missed: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamNotification {
    pub stream: StreamKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    pub seq: u64,
    pub payload: StreamPayload,
}
