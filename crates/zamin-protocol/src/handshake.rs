//! Mandatory first exchange (protocol spec §2).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloParams {
    pub protocol: u32,
    /// Opaque in v0: local transports ignore it; a remote transport will
    /// require it. This is the authentication hook — nothing more (ADR-0002).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    pub client: ClientInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonInfo {
    pub name: String,
    pub version: String,
}

/// Feature capabilities the daemon supports, so clients can degrade cleanly
/// instead of guessing.
pub mod capabilities {
    pub const SERVER_LIFECYCLE: &str = "server.lifecycle";
    pub const STREAMS: &str = "streams";
    pub const JOBS: &str = "jobs";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloResult {
    pub protocol: u32,
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub daemon: DaemonInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}
