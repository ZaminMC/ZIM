//! The join check (§7's completion): the daemon classifies a join
//! address the operator typed. A Minecraft server browser answers with
//! the server's verdict — alive (with the status response's facts),
//! refused, unreachable, timed out, or "not a Minecraft server" — never
//! with a raw webview connection error.

use serde::{Deserialize, Serialize};

/// What the panel asked about. `host` is `None` for the port-only
/// dialect (`:25565`) — the daemon reads the local loopback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinCheckParams {
    pub host: Option<String>,
    pub port: u16,
}

/// The classified verdict, from the server-list ping's own failure
/// modes:
/// - `alive`: a Minecraft server answered (the status facts ride along);
/// - `refused`: nothing is listening on the port;
/// - `timeout`: the address stopped answering mid-exchange;
/// - `unreachable`: the host could not be reached (no route, no DNS);
/// - `invalid`: something answered, but not the Minecraft protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinState {
    Alive,
    Refused,
    Timeout,
    Unreachable,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinCheckResult {
    pub state: JoinState,
    pub motd: Option<String>,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    pub version: Option<String>,
}
