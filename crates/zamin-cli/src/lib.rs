//! `zamin` client library: the second protocol client (Phase 2). Exposed
//! as a library so integration tests can drive the real client code path
//! against the real daemon binary.

mod client;

pub use client::{Client, ClientError, CLIENT_NAME, CLIENT_VERSION};
