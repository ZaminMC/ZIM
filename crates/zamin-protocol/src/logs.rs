//! `logs.range` — the file-backed historical read (protocol spec §5, §6).
//!
//! Streams deliver what the daemon has ingested; the log *files* hold the
//! full history. This method reads a server's `logs/latest.log` directly,
//! through the rooted filesystem, so a client can scroll back past what any
//! ring or subscription can serve (ADR-0006: catch up through file-backed
//! APIs).
//!
//! Pages are addressed by a byte-offset cursor, mirroring the logs stream's
//! `{file, offset}` cursor model (spec §6): a response reports the byte
//! offset where its first returned line starts, and the client pages back
//! by sending that offset as `beforeOffset`. The read walks the file
//! backward in bounded windows, so a page never loads the whole file and a
//! tail never re-reads from offset 0 (PERFORMANCE-BUDGETS).
//!
//! Lines keep the [`crate::streams::LogLine`] shape. `tsMs` is the
//! ingestion time, which file-backed lines never had — it is 0, and the raw
//! JVM timestamp stays visible in the file itself (protocol spec §9: no
//! false precision).

use serde::{Deserialize, Serialize};

use crate::streams::LogLine;

/// How many lines a single `logs.range` call may return. Bounds the
/// response frame; clients wanting more scroll back in pages.
pub const LOG_RANGE_MAX_LINES: u32 = 5_000;

/// Backward-read window: the reader walks the file in windows of at most
/// this many bytes, starting at the cursor (or the end of the file) and
/// stepping toward the start until it has the requested lines. Bounds the
/// read amplification and memory of one call.
pub const LOG_RANGE_WINDOW_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRangeParams {
    pub server_id: String,
    /// Lines to return. Defaults to 200; capped at [`LOG_RANGE_MAX_LINES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u32>,
    /// Byte-offset cursor: return the last `maxLines` lines that END at or
    /// before this byte offset in the log file. Absent — read from the end
    /// of the file (the tail). To page backward, pass the previous
    /// response's `startOffset`. A cursor beyond the current file length
    /// (rotation or truncation happened) is a typed
    /// `LOG_CURSOR_INVALID`; the client restarts from the tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRangeResult {
    /// The file the lines came from, server-root-relative.
    pub file: String,
    /// Chronological; at most `maxLines` lines, each ending at or before
    /// the requested offset (or the end of the file).
    pub lines: Vec<LogLine>,
    /// True when the file holds older lines than this response returned —
    /// a scroll-up affordance, not an error. Exactly `startOffset > 0`.
    pub older_available: bool,
    /// Byte offset of the start of the first returned line — a line
    /// boundary. Pass it as `beforeOffset` to page further back; when it
    /// is 0 the file has no older lines.
    pub start_offset: u64,
}
