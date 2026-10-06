//! `players.list` — who is on a server right now (protocol spec §5). The
//! daemon asks the server itself with a Server List Ping (vanilla flow,
//! no plugins needed); an unreachable server is a result shape, not an
//! error — "nobody, because it is off" is a normal answer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayersListParams {
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSample {
    pub name: String,
    /// The server-reported player UUID, when the status carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Where this listing came from. v0 ships the Server List Ping; the log
/// join/leave roster rides along in `roster` (the ping sample caps at 12
/// names; the log does not).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayersSource {
    Ping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayersListResult {
    /// "ping" — the daemon asked the server directly.
    pub source: PlayersSource,
    /// The server answered: these describe the live session.
    pub online: Option<u32>,
    pub max: Option<u32>,
    /// The server's own preview of who is on (vanilla caps the sample at
    /// 12 names — not the full roster).
    pub sample: Vec<PlayerSample>,
    /// The log-roster: players whose join lines have not been followed by
    /// a leave. Empty unless the daemon's log pumps saw a join — adopted
    /// servers report nothing here until they are restarted by this
    /// daemon (honest emptiness, not a guess).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roster: Vec<PlayerSample>,
    /// Round-trip latency of the status exchange, milliseconds.
    pub latency_ms: u32,
    /// The server's version string, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The message of the day, flattened to plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motd: Option<String>,
}
