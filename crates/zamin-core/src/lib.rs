//! ZaminCore — the engine that owns servers: registry, configuration, the
//! filesystem safety model, process supervision, Java discovery, ports, and
//! the log pipeline.
//!
//! Only `zamind` links this crate. Clients see the engine exclusively
//! through the Zamin Protocol (ADR-0002); nothing in here may be reached
//! around. Platform-specific behavior is confined to `platform` (ADR-0008).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod config;
pub mod error;
pub mod fsops;
pub mod java;
pub mod logparse;
pub mod net;
pub mod ping;
pub mod platform;
pub mod server;
pub mod supervisor;
