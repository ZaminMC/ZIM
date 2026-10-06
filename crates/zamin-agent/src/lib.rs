//! ZaminAgent — the remote transport for the Zamin Protocol (ADR-0011).
//!
//! ADR-0001 reserved the agent as a protocol client: it connects to the
//! local daemon over the per-user transport and serves remote protocol
//! clients over TLS. The daemon stays purely local; the agent is the only
//! network-facing process. It understands frames and the first exchange —
//! no server or job semantics live here.
//!
//! Security model (ADR-0011):
//! - TLS 1.3 with a self-signed certificate generated once per install;
//!   remote clients pin the certificate's SHA-256 fingerprint.
//! - The token is the credential: the first frame must be `daemon.hello`
//!   with `auth` matching the agent's token (constant-time compared)
//!   before any local daemon connection is made.
//! - The original hello is forwarded unchanged; the daemon ignores `auth`
//!   on local transports, by spec.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod auth;
pub mod client;
pub mod relay;
pub mod server;
pub mod tls;

use zamin_ipc::IpcError;
use zamin_protocol::error::ProtocolError;

/// Fixed-time equality over byte slices. Both inputs here are 32-byte
/// SHA-256 digests, so the length check compares public information only.
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("tls material: {0}")]
    Tls(#[from] tls::TlsError),
    #[error("local daemon connection: {0}")]
    Ipc(#[from] IpcError),
    #[error("{0}")]
    Protocol(#[from] ProtocolError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("frame encoding: {0}")]
    Frame(#[from] serde_json::Error),
    #[error("malformed fingerprint ({0} chars expected as sha-256 hex)")]
    Fingerprint(usize),
}
