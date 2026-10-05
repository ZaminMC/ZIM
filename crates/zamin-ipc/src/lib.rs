//! Local transports for the Zamin Protocol: framed connections over a
//! per-user named pipe (Windows) or Unix domain socket (Linux).
//!
//! This crate is one of the two places OS-specific code may live (ADR-0008);
//! it contains no business logic and knows nothing about servers or jobs.
//! Framing itself is `zamin-protocol::framing` — this crate only moves
//! framed bytes.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod client;
pub mod connection;
pub mod endpoint;
pub mod error;
pub mod server;

#[cfg(windows)]
#[path = "platform_windows.rs"]
mod platform;
#[cfg(unix)]
#[path = "platform_unix.rs"]
mod platform;

pub use client::connect;
pub use connection::{Connection, ConnectionReadHalf, ConnectionWriteHalf};
pub use endpoint::Endpoint;
pub use error::IpcError;
pub use server::IpcServer;
