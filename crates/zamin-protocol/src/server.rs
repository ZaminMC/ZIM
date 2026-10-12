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
    /// The pattern-matched failure family (zamin-core's crash table,
    /// 2026-10-12): present only when the evidence names a known cause —
    /// a vanilla-jar DNS failure, a port conflict, a heap shortfall —
    /// and always derived from the real process output, never guessed.
    /// Older daemons omit it; the card's raw-excerpt fallback covers
    /// that era unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnosis: Option<CrashDiagnosis>,
}

/// One diagnosed failure family: a stable kind (the wire's kebab-case
/// id), a human title, and remediation steps the operator can walk.
/// Defined HERE because it is wire data (the crash card's payload);
/// the pattern table that produces it lives in the engine
/// (zamin-core's `crash` module) and may evolve independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDiagnosis {
    /// The family's stable id — the wire contract for UI styling.
    pub kind: CrashDiagnosisKind,
    /// The one sentence the crash card leads with.
    pub title: String,
    /// Ordered, actionable steps (each its own line in the card).
    pub remediation: Vec<String>,
}

/// The failure families the engine's pattern table knows. Kebab-case
/// on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrashDiagnosisKind {
    /// DNS resolution failed during a download the server itself
    /// performs (Paperclip's vanilla jar, plugin fetches, Mojang APIs).
    NetworkDns,
    /// A connection timed out or was refused mid-download.
    NetworkTimeout,
    /// The server's port was already taken (BindException).
    PortInUse,
    /// The JVM could not get the memory it asked for.
    Memory,
    /// The configured Java is too old for the server software.
    JavaVersion,
    /// The jar itself is corrupt or not a server jar.
    CorruptJar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSummary {
    pub server_id: String,
    pub display_name: String,
    pub state: ServerState,
    /// The address the server answers on: the layered config's desired
    /// port override, else the boot authority in `server.properties`
    /// (ADR-0007). None = the server has no known port yet — the address
    /// bar cannot resolve it, and says so instead of guessing.
    /// The address resolver consumes this (P0: `0:25565` must reach the
    /// server that owns the port; a server id is an identifier, never an
    /// address).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// `server.properties`'s `server-ip` — the interface the server binds.
    /// None/empty = all interfaces; a specific value must equal the typed
    /// host for the address to resolve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,
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
