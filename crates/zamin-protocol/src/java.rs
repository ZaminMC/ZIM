//! Java runtimes (Phase 6): the discovered/managed runtime list and the
//! install job that fetches a Temurin JDK from the Adoptium API
//! (protocol spec §7c).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `java.list` — every runtime the daemon can start a server with:
/// discovered on PATH/JAVA_HOME/install roots, plus anything the daemon
/// itself fetched into its managed directory (`managed: true`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaListResult {
    pub runtimes: Vec<JavaRuntime>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntime {
    /// Absolute path to the `java` executable. The one field clients may
    /// rely on; names and directories prove nothing (ADR-0005 preflight).
    pub path: String,
    pub major: u32,
    pub version_string: String,
    pub vendor: String,
    /// True when the daemon fetched this runtime itself (Adoptium).
    pub managed: bool,
}

/// `java.install {majorVersion}` — a job: resolve the newest Temurin GA
/// JDK for this machine, download and verify it, extract it into the
/// daemon's managed directory, inspect the result. Idempotent for an
/// already-installed release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallParams {
    pub request_id: Uuid,
    pub major_version: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallResult {
    pub kind: crate::jobs::JobKind,
    pub job: crate::jobs::Job,
}
