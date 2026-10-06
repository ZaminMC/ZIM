//! Backup and restore of server roots as tar.gz archives (ADR-0009):
//! containment-checked walks, staging + atomic commit, the full
//! cross-platform restore trap list (zip-slip, Windows-reserved names,
//! case-insensitive collisions, size limits), typed disk-full, and
//! retention pruning. Sync on purpose — the daemon drives it through
//! `spawn_blocking` inside cancellable jobs.
//!
//! Nothing here talks to the daemon: callers pass explicit paths
//! (the server root and the backups directory) so the data-dir layout
//! stays a daemon concern.

mod create;
mod restore;

#[cfg(test)]
mod tests;

pub use create::{create_archive, BackupCreateOptions, BackupCreateOutcome, CreateProgress};
pub use restore::{
    restore_archive, RestoreOptions, RestoreOutcome, RestoreProgress, RESTORE_MAX_ENTRIES,
    RESTORE_MAX_TOTAL_BYTES,
};

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Bump when the archive/manifest shape ever changes; restore refuses
/// (typed error) formats it does not understand rather than guessing.
pub const ARCHIVE_FORMAT_VERSION: u32 = 1;

/// Sidecar JSON written next to every archive; `backups.list` reads these.
/// The file name is `<backupId>.json`; the archive is `<backupId>.tar.gz`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub format_version: u32,
    pub backup_id: uuid::Uuid,
    pub server_id: String,
    pub created_at_ms: i64,
    /// Compressed archive size on disk.
    pub size_bytes: u64,
    /// Uncompressed bytes the archive holds (sum of file sizes walked).
    pub total_bytes: u64,
    pub file_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// How the backup was taken: "live" (save-off/save-all window around
    /// a running server) or "cold" (server not running).
    pub taken: BackupTaken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackupTaken {
    Live,
    Cold,
}

/// Directories that must never enter an archive or a restore: working
/// staging areas of the file manager and of concurrent restore/backup
/// operations. The daemon's own state lives outside the root and needs no
/// exclusion; `.zamin/` (the identity marker) is deliberately INCLUDED —
/// it is part of the server.
pub fn is_excluded(top_level: &str) -> bool {
    top_level == ".zamin-staging"
        || top_level.starts_with(".zamin-backup-staging-")
        || top_level.starts_with(".zamin-restore-staging-")
        || top_level.starts_with(".zamin-restore-backup-")
}

/// Read every manifest in a backups directory, oldest first. Archives
/// without a readable manifest are skipped (a crashed staging write never
/// poisons the list); the daemon surfaces them as orphans if it cares.
pub fn list_backups(backups_dir: &Path) -> Vec<BackupManifest> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(backups_dir) else {
        return out;
    };
    for item in read.flatten() {
        let path = item.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(manifest) = serde_json::from_slice::<BackupManifest>(&bytes) {
                    out.push(manifest);
                }
            }
        }
    }
    out.sort_by_key(|m| m.created_at_ms);
    out
}

/// Retention (ARCH-REVIEW §16.5): keep the newest `keep` backups, delete
/// the rest (archive + manifest). Returns the pruned ids, oldest first.
/// A missing file is tolerated — retention is janitorial, never fatal.
pub fn prune_backups(backups_dir: &Path, keep: usize) -> std::io::Result<Vec<uuid::Uuid>> {
    let mut backups = list_backups(backups_dir);
    if backups.len() <= keep {
        return Ok(Vec::new());
    }
    let excess = backups.len() - keep;
    let pruned: Vec<BackupManifest> = backups.drain(..excess).collect();
    let mut ids = Vec::with_capacity(pruned.len());
    for manifest in pruned {
        let archive = archive_path(backups_dir, manifest.backup_id);
        let manifest_path = manifest_path(backups_dir, manifest.backup_id);
        if archive.exists() {
            std::fs::remove_file(&archive)?;
        }
        if manifest_path.exists() {
            std::fs::remove_file(&manifest_path)?;
        }
        ids.push(manifest.backup_id);
    }
    Ok(ids)
}

pub fn archive_path(backups_dir: &Path, backup_id: uuid::Uuid) -> PathBuf {
    backups_dir.join(format!("{backup_id}.tar.gz"))
}

pub fn manifest_path(backups_dir: &Path, backup_id: uuid::Uuid) -> PathBuf {
    backups_dir.join(format!("{backup_id}.json"))
}

/// The staging file for archive creation (inside the backups dir, so a
/// crashed write never leaves partial archives in the listing).
pub fn create_staging_path(backups_dir: &Path, backup_id: uuid::Uuid) -> PathBuf {
    backups_dir.join(format!(".zamin-backup-staging-{backup_id}.tar.gz"))
}

/// The staging directory a restore extracts into, inside the server root
/// (ADR-0009: staging + commit/rollback; everything stays on one
/// filesystem so renames are atomic).
pub fn restore_staging_path(root: &Path, backup_id: uuid::Uuid) -> PathBuf {
    root.join(format!(".zamin-restore-staging-{backup_id}"))
}

/// The rollback directory a restore moves the current root contents into
/// before committing the staged tree.
pub fn restore_rollback_path(root: &Path, backup_id: uuid::Uuid) -> PathBuf {
    root.join(format!(".zamin-restore-backup-{backup_id}"))
}

/// Map an io::Error to the typed disk-full error when the OS says the
/// disk is full: ENOSPC (28) on Linux, ERROR_DISK_FULL (112) on Windows,
/// EDQUOT (122) when a quota is enforced on Linux. Everything else is
/// passed through as a plain Io error.
pub fn classify_io(path: PathBuf, source: io::Error) -> crate::error::CoreError {
    if is_disk_full(&source) {
        crate::error::CoreError::DiskFull { path }
    } else {
        crate::error::CoreError::Io { path, source }
    }
}

fn is_disk_full(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(28) | Some(112) | Some(122))
}
