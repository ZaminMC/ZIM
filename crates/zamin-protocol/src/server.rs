//! Server domain types and method params/results (protocol spec §5).
//!
//! Servers are referenced by `serverId`, never by paths. The one exception is
//! `server.register`, whose `rootPath` bootstraps the binding itself
//! (ADR-0004); afterwards only the daemon knows the location.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Lifecycle state of a server (ADR-0005). Attachment is a separate
/// dimension and is not a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServerState {
    NotRunning,
    Starting,
    Running,
    Stopping,
    Stopped,
    FailedPreflight,
    Crashed,
    Adopting,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrashPhase {
    Startup,
    Runtime,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashClassification {
    pub phase: CrashPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Evidence excerpt (last log activity) — display data, not for parsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSummary {
    pub server_id: String,
    pub display_name: String,
    pub state: ServerState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDetails {
    pub server_id: String,
    pub display_name: String,
    pub state: ServerState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub software: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

// --- params / results ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterServerParams {
    pub request_id: Uuid,
    pub server_id: String,
    pub display_name: String,
    /// The bootstrap exception to the no-paths rule (ADR-0004).
    pub root_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterServerResult {
    pub server: ServerDetails,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetServerParams {
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateServerParams {
    pub request_id: Uuid,
    pub server_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveServerParams {
    pub request_id: Uuid,
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerIdParams {
    pub request_id: Uuid,
    pub server_id: String,
}

/// One console line to the server's stdin (terminal input). No response
/// body beyond acceptance; output arrives on the logs stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StdinParams {
    pub request_id: Uuid,
    pub server_id: String,
    pub line: String,
}

/// Result of start/stop/restart/kill: the accepted state transition has begun.
/// Outcomes arrive as `server.state_changed` events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleResult {
    pub server_id: String,
    pub state: ServerState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListServersResult {
    pub servers: Vec<ServerSummary>,
}

/// Helper for methods whose result carries no fields beyond a success
/// marker; serialized as `{}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyResult {}

/// Convenience for params that are only a `request_id` + free payload (used
/// sparingly; prefer named params structs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawParams(pub Value);
