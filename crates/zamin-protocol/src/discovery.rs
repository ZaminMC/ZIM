//! Server discovery on the wire (founder §64, ADR-0027): the daemon's
//! answer to "what servers does this machine already have". The result is
//! a merged, typed list — managed servers first (with their live state),
//! then unregistered server directories and standalone supported jars —
//! plus the honesty fields: which configured roots were scanned, which
//! could not be read, and whether the walk hit its budget.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverParams {
    /// Substring filter over id, name, and path (case-insensitive). Absent
    /// or empty = everything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// One discovered thing. `kind` is a closed set: `registered` (a managed
/// server — id and state ride along), `directory` (a server-shaped
/// directory on disk), `jar` (a standalone supported jar).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredServer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Absolute path: the server root (registered/directory) or the jar
    /// file itself.
    pub path: String,
    pub kind: String,
    /// Live lifecycle state, for registered servers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// From server.properties, for directory candidates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The jar family the filename classified as (evidence, not a probe).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// The marker-bound server id, when the directory carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// For jar candidates: the file name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jar_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverResult {
    pub servers: Vec<DiscoveredServer>,
    /// The roots actually scanned (configured roots + the instance dir).
    pub roots: Vec<String>,
    /// Configured roots the OS refused or that are absent — named, not
    /// silently dropped.
    pub skipped_roots: Vec<String>,
    pub scanned: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootsGetResult {
    /// The operator-configured scan roots (the instance dir is implicit
    /// and always scanned, so it is not listed here).
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootsSetParams {
    pub roots: Vec<String>,
}
