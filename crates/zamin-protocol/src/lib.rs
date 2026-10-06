//! Zamin Protocol v0 — the boundary between clients (Panel, CLI, future
//! agent) and the daemon.
//!
//! This crate is pure data and codec: no I/O, no OS dependencies. Transports
//! live in `zamin-ipc`; the engine lives in `zamin-core`. Both sides of a
//! connection depend only on this crate, which is what makes the local and
//! remote transports the same protocol (ADR-0002).
//!
//! Readers are tolerant: unknown JSON fields are ignored, which is what makes
//! additive protocol evolution free.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod backups;
pub mod envelope;
pub mod error;
pub mod files;
pub mod framing;
pub mod handshake;
pub mod java;
pub mod jobs;
pub mod logs;
pub mod methods;
pub mod players;
pub mod server;
pub mod software;
pub mod streams;

/// Wire protocol version. Additive changes do not bump it; breaking changes
/// bump it and define a min/max negotiation window (protocol spec §2).
pub const PROTOCOL_VERSION: u32 = 1;

pub use error::{ErrorCode, ProtocolError};
