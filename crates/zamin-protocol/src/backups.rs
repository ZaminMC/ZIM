//! Backups surface: create/restore as jobs (protocol spec §7), plus the
//! manifest-backed listing. Creation and restore never block the caller —
//! they return the running `Job` immediately; progress arrives as
//! `job.started / job.progress / job.completed` events on the events
//! stream.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::jobs::{Job, JobKind};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCreateParams {
    pub request_id: Uuid,
    pub server_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCreateResult {
    pub kind: JobKind,
    pub job: Job,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestoreParams {
    pub request_id: Uuid,
    pub server_id: String,
    pub backup_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestoreResult {
    pub kind: JobKind,
    pub job: Job,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupsListParams {
    pub server_id: String,
}

/// One backup in the list, from the daemon-side manifest. Newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub backup_id: Uuid,
    pub created_at_ms: i64,
    /// Compressed archive size on disk.
    pub size_bytes: u64,
    /// Uncompressed bytes the archive holds.
    pub total_bytes: u64,
    pub file_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub taken: crate::jobs::BackupTaken,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupsListResult {
    pub backups: Vec<BackupInfo>,
}
