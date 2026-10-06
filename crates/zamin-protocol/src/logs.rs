//! `logs.range` — the file-backed historical read (protocol spec §5, §6).
//!
//! Streams deliver what the daemon has ingested; the log *files* hold the
//! full history. This method tails a server's `logs/latest.log` directly,
//! through the rooted filesystem, so a client can scroll back past what any
//! ring or subscription can serve (ADR-0006: catch up through file-backed
//! APIs).
//!
//! Lines keep the [`crate::streams::LogLine`] shape. `tsMs` is the
//! ingestion time, which file-backed lines never had — it is 0, and the raw
//! JVM timestamp stays visible in the file itself (protocol spec §9: no
//! false precision).

use serde::{Deserialize, Serialize};

use crate::streams::LogLine;

/// How far back a single `logs.range` call may reach. Bounds the response
/// frame; clients wanting more scroll back in pages.
pub const LOG_RANGE_MAX_LINES: u32 = 5_000;

/// Tail window: when a log file is larger than this, only its trailing
/// window is scanned for the requested lines. Generous enough that any
/// sane `maxLines` is served from it in full.
pub const LOG_RANGE_WINDOW_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRangeParams {
    pub server_id: String,
    /// Lines to return from the end of the file. Defaults to 200; capped
    /// at [`LOG_RANGE_MAX_LINES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRangeResult {
    /// The file the lines came from, server-root-relative.
    pub file: String,
    /// Chronological; the last `maxLines` lines of the file.
    pub lines: Vec<LogLine>,
    /// True when the file holds older lines than this response returned —
    /// a scroll-up affordance, not an error.
    pub older_available: bool,
}
