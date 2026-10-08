//! Backup creation: walk the server root into a tar.gz archive, staged
//! then atomically committed into the backups dir with a sidecar manifest.
//!
//! Safety properties (ADR-0009):
//! - The walk never follows symlinks (a link pointing outside the root
//!   cannot leak data into the archive; links are skipped entirely).
//! - Entry names come from `read_dir`, so they cannot contain `..` or
//!   separators; containment is structural, not checked per entry.
//! - The archive is written to a staging file first; a crash or a disk-full
//!   mid-write never leaves a half archive in the listing. Disk-full is a
//!   typed error, not a generic IO one.
//! - Cancellation is observed between files; the staging file is cleaned.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    archive_path, classify_io, create_staging_path, BackupManifest, BackupTaken,
    ARCHIVE_FORMAT_VERSION,
};
use crate::error::CoreError;

/// Emitted once per file (and once at the end for the archive flush).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateProgress {
    pub files_done: u64,
    pub bytes_done: u64,
}

/// Owned, shareable options: the daemon runs archive work inside
/// `spawn_blocking`, so callbacks and the cancel flag must be `'static`.
pub struct BackupCreateOptions {
    pub server_id: String,
    pub label: Option<String>,
    pub taken: BackupTaken,
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<dyn Fn(CreateProgress) + Send + Sync>,
}

pub struct BackupCreateOutcome {
    pub backup_id: uuid::Uuid,
    pub archive: PathBuf,
    pub manifest: BackupManifest,
}

impl std::fmt::Debug for BackupCreateOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupCreateOutcome")
            .field("backup_id", &self.backup_id)
            .field("archive", &self.archive)
            .field("manifest", &self.manifest)
            .finish()
    }
}

/// Create a backup of `root` inside `backups_dir`. The caller owns the
/// directories' existence (the daemon creates `backups_dir` up front);
/// the staging file and the final manifest are managed here.
pub fn create_archive(
    root: &Path,
    backups_dir: &Path,
    opts: BackupCreateOptions,
) -> Result<BackupCreateOutcome, CoreError> {
    if !root.is_dir() {
        return Err(CoreError::NotFound {
            path: root.to_path_buf(),
        });
    }
    fs::create_dir_all(backups_dir).map_err(|source| classify_io(backups_dir.into(), source))?;

    let backup_id = uuid::Uuid::now_v7();
    let staging = create_staging_path(backups_dir, backup_id);

    match write_archive(root, &staging, &opts) {
        Ok(stats) => commit(backups_dir, &staging, backup_id, opts, stats),
        Err(e) => {
            let _ = fs::remove_file(&staging);
            Err(e)
        }
    }
}

struct ArchiveStats {
    file_count: u64,
    total_bytes: u64,
}

fn cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

fn write_archive(
    root: &Path,
    staging: &Path,
    opts: &BackupCreateOptions,
) -> Result<ArchiveStats, CoreError> {
    let file =
        fs::File::create(staging).map_err(|source| classify_io(staging.to_path_buf(), source))?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    let mut builder = tar::Builder::new(gz);

    let mut stats = ArchiveStats {
        file_count: 0,
        total_bytes: 0,
    };
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancelled(&opts.cancel) {
            return Err(CoreError::Cancelled);
        }
        let mut entries: Vec<fs::DirEntry> = fs::read_dir(&dir)
            .map_err(|source| classify_io(dir.clone(), source))?
            .collect::<Result<_, _>>()
            .map_err(|source| classify_io(dir.clone(), source))?;
        // Sorted for deterministic archives; names come from read_dir and
        // cannot contain path separators or `..`.
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            if cancelled(&opts.cancel) {
                return Err(CoreError::Cancelled);
            }
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();

            // Staging dirs of concurrent operations never enter an archive;
            // they only ever live at the root.
            if dir == root && super::is_excluded(&name) {
                continue;
            }

            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");

            // Symlinks are skipped, never followed (see module docs).
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_symlink() {
                continue;
            }

            if meta.is_dir() {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                header.set_mtime(mtime_secs(&meta));
                // append_data names the entry from `rel`, writing GNU
                // long-name extensions for overlong paths itself.
                builder
                    .append_data(&mut header, &rel, std::io::empty())
                    .map_err(|source| classify_io(staging.to_path_buf(), source))?;
                stack.push(path);
            } else if meta.is_file() {
                let mut header = tar::Header::new_gnu();
                header.set_size(meta.len());
                header.set_mode(0o644);
                header.set_mtime(mtime_secs(&meta));
                let mut f =
                    fs::File::open(&path).map_err(|source| classify_io(path.clone(), source))?;
                builder
                    .append_data(&mut header, &rel, &mut f)
                    .map_err(|source| classify_io(staging.to_path_buf(), source))?;
                stats.file_count += 1;
                stats.total_bytes += meta.len();
                (opts.progress)(CreateProgress {
                    files_done: stats.file_count,
                    bytes_done: stats.total_bytes,
                });
            }
            // Anything else (sockets, fifos, devices) is not server data.
        }
    }

    let gz = builder
        .into_inner()
        .map_err(|source| classify_io(staging.to_path_buf(), source))?;
    // fsync rides the write handle before it is dropped — Windows answers
    // ACCESS_DENIED to FlushFileBuffers on a read-only reopen, so the
    // commit below renames an already-durable archive without reopening.
    let file = gz
        .finish()
        .map_err(|source| classify_io(staging.to_path_buf(), source))?;
    file.sync_all()
        .map_err(|source| classify_io(staging.to_path_buf(), source))?;
    drop(file);
    Ok(stats)
}

fn commit(
    backups_dir: &Path,
    staging: &Path,
    backup_id: uuid::Uuid,
    opts: BackupCreateOptions,
    stats: ArchiveStats,
) -> Result<BackupCreateOutcome, CoreError> {
    // The archive is complete and already fsynced by write_archive (on its
    // write handle — a read-only reopen cannot FlushFileBuffers on Windows).
    let archive = archive_path(backups_dir, backup_id);
    fs::rename(staging, &archive).map_err(|source| classify_io(archive.clone(), source))?;

    let manifest = BackupManifest {
        format_version: ARCHIVE_FORMAT_VERSION,
        backup_id,
        server_id: opts.server_id.clone(),
        created_at_ms: now_ms(),
        size_bytes: fs::metadata(&archive).map(|m| m.len()).unwrap_or_default(),
        total_bytes: stats.total_bytes,
        file_count: stats.file_count,
        label: opts.label,
        taken: opts.taken,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| {
        let _ = fs::remove_file(&archive);
        CoreError::Io {
            path: archive.clone(),
            source: std::io::Error::other(format!("manifest serialization failed: {e}")),
        }
    })?;
    if let Err(e) = crate::fsops::atomic_write(
        &super::manifest_path(backups_dir, backup_id),
        &manifest_bytes,
    ) {
        let _ = fs::remove_file(&archive);
        return Err(e);
    }

    Ok(BackupCreateOutcome {
        backup_id,
        archive,
        manifest,
    })
}

fn mtime_secs(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
