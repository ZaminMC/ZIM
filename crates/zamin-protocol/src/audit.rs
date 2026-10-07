//! The audit's read side (ADR-0011). The audit is written by the daemon
//! as one JSONL line per handshake and per mutating command; `audit.list`
//! reads it back newest-first, paged, without ever rewriting it. The wire
//! shape mirrors the writer's line field-for-field — `tsMs`, `method`,
//! `serverId?`, `outcome`, `client?` — because the file is the schema:
//! the reader parses what the writer appended, and a line the file cannot
//! answer for is counted (`malformed`), never silently dropped.

use serde::{Deserialize, Serialize};

/// `audit.list {limit?, offset?}` — newest-first paging: `offset` counts
/// entries back from the newest (0 = the page starts at the newest line).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

/// The protocol client recorded at the handshake — the actor as honest as
/// the daemon can name it (the agent gates remote identities first).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditClient {
    pub name: String,
    pub version: String,
}

/// One audit line, read back. Mirrors the writer's JSON field-for-field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub ts_ms: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    /// "ok" or the protocol error code the daemon answered with.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<AuditClient>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditListResult {
    /// Newest first. `entries.len() <= limit`.
    pub entries: Vec<AuditEntry>,
    /// Older entries exist beyond this page.
    pub has_more: bool,
    /// Lines on disk that did not parse as audit JSON. They stay on disk;
    /// the listing counts them instead of pretending they are not there.
    #[serde(default)]
    pub malformed: u32,
}
