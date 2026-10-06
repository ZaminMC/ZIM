//! Backup restore: extract a tar.gz archive over a server root with every
//! cross-platform trap from ADR-0009 handled as a typed failure:
//!
//! - zip-slip: entry names are validated to be relative, separator-clean,
//!   and dot-free before anything is written; extraction targets are built
//!   by joining validated components only.
//! - Windows-reserved names (`CON`, `NUL`, `COM1`…): rejected on every
//!   path component, case-insensitively, extension or not — an archive
//!   made on Linux must restore on Windows without data loss.
//! - Case-insensitive collisions (`World` vs `world`): rejected, because
//!   the target platform's filesystem may be case-folded.
//! - Total-size and entry-count limits; hard/symlinks and device entries
//!   are rejected (they are not server data and cannot be restored safely
//!   cross-platform).
//! - Disk-full is a typed error.
//!
//! The extract goes into a staging dir inside the root; commit moves the
//! live tree aside (rollback dir) and the staged tree into place with
//! renames. Any commit failure rolls the previous files back — the worst
//! case is "nothing changed", never "half restored".

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{classify_io, restore_rollback_path, restore_staging_path};
use crate::error::CoreError;

/// Safety limits for restore (ADR-0009). A malicious or corrupt archive
/// cannot balloon into unbounded extraction; a limit hit is a typed
/// failure. Generous for real servers.
pub const RESTORE_MAX_ENTRIES: u64 = 200_000;
pub const RESTORE_MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024 * 1024; // 32 GiB

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreProgress {
    pub entries_done: u64,
    pub bytes_done: u64,
}

pub struct RestoreOptions<'a> {
    pub cancel: &'a AtomicBool,
    pub progress: &'a dyn Fn(RestoreProgress),
    /// 0 means the built-in [`RESTORE_MAX_ENTRIES`].
    pub max_entries: u64,
    /// 0 means the built-in [`RESTORE_MAX_TOTAL_BYTES`].
    pub max_total_bytes: u64,
}

#[derive(Debug)]
pub struct RestoreOutcome {
    pub restored_files: u64,
    pub restored_bytes: u64,
}

/// Windows reserves these device names on every filesystem, with or
/// without an extension, in any case, with trailing dots/spaces trimmed.
/// Restoring an entry with such a name on Windows fails or, worse, writes
/// into the device namespace.
const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Restore `archive` over the server root `root`. The root's current
/// contents are replaced with the archive's; on any commit failure the
/// previous contents are rolled back. The server must be stopped (the
/// daemon enforces this; locked files on Windows would fail the commit).
pub fn restore_archive(
    root: &Path,
    archive: &Path,
    opts: &RestoreOptions<'_>,
) -> Result<RestoreOutcome, CoreError> {
    if !root.is_dir() {
        return Err(CoreError::NotFound {
            path: root.to_path_buf(),
        });
    }
    if !archive.is_file() {
        return Err(CoreError::NotFound {
            path: archive.to_path_buf(),
        });
    }

    // Transient workspace names; a fresh uuid avoids collisions with any
    // other operation and is never recorded anywhere.
    let workspace = uuid::Uuid::now_v7();
    let staging = restore_staging_path(root, workspace);
    let rollback = restore_rollback_path(root, workspace);

    let stats = match extract_into(archive, &staging, opts) {
        Ok(stats) => stats,
        Err(e) => {
            // Extraction failures leave the live root untouched.
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    };

    match commit(root, &staging, &rollback) {
        Ok(()) => Ok(RestoreOutcome {
            restored_files: stats.files,
            restored_bytes: stats.bytes,
        }),
        Err(CommitError::RolledBack(reason)) => Err(CoreError::RestoreRolledBack { reason }),
        Err(CommitError::RollbackBroken(reason)) => Err(CoreError::RestoreRolledBack {
            reason: format!("{reason}; the rollback itself failed — inspect the server directory"),
        }),
    }
}

#[derive(Debug)]
struct ExtractStats {
    files: u64,
    bytes: u64,
}

fn extract_into(
    archive: &Path,
    staging: &Path,
    opts: &RestoreOptions<'_>,
) -> Result<ExtractStats, CoreError> {
    let file =
        fs::File::open(archive).map_err(|source| classify_io(archive.to_path_buf(), source))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);

    fs::create_dir_all(staging).map_err(|source| classify_io(staging.to_path_buf(), source))?;

    let max_entries = if opts.max_entries == 0 {
        RESTORE_MAX_ENTRIES
    } else {
        opts.max_entries
    };
    let max_total = if opts.max_total_bytes == 0 {
        RESTORE_MAX_TOTAL_BYTES
    } else {
        opts.max_total_bytes
    };

    let mut seen_lower: HashSet<String> = HashSet::new();
    let mut total_bytes: u64 = 0;
    let mut entries_count: u64 = 0;
    let mut stats = ExtractStats { files: 0, bytes: 0 };

    let entries = tar
        .entries()
        .map_err(|source| CoreError::ArchiveUnsafeEntry {
            entry: "<archive>".to_owned(),
            reason: format!("the archive is unreadable: {source}"),
        })?;

    for entry in entries {
        if opts.cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let mut entry = entry.map_err(|source| CoreError::ArchiveUnsafeEntry {
            entry: "<entry>".to_owned(),
            reason: format!("the next archive entry is unreadable: {source}"),
        })?;

        // Raw name bytes: validation happens on exactly what the archive
        // says, before any path building.
        let raw = entry.path_bytes();
        let name = std::str::from_utf8(&raw).map_err(|_| CoreError::ArchiveUnsafeEntry {
            entry: String::from_utf8_lossy(&raw).into_owned(),
            reason: "entry names must be UTF-8".to_owned(),
        })?;
        let rel = validate_name(name)?;

        if rel.is_empty() {
            continue; // the root entry itself ("/")
        }
        if super::is_excluded(rel.split('/').next().unwrap_or_default()) {
            continue; // staging dirs of concurrent operations
        }

        // Case-insensitive collision: the archive may target a
        // case-folding filesystem (macOS, most Windows filesystems).
        let lower = rel.to_ascii_lowercase();
        if !seen_lower.insert(lower) {
            return Err(CoreError::ArchiveUnsafeEntry {
                entry: rel.to_owned(),
                reason: "two entries collide case-insensitively".to_owned(),
            });
        }

        entries_count += 1;
        let size = entry.size();
        total_bytes += size;
        if entries_count > max_entries || total_bytes > max_total {
            return Err(CoreError::ArchiveTooLarge {
                found: total_bytes,
                entries: entries_count,
                max_bytes: max_total,
                max_entries,
            });
        }

        let target = staging.join(rel);
        match entry.header().entry_type() {
            tar::EntryType::Directory => {
                fs::create_dir_all(&target)
                    .map_err(|source| classify_io(target.clone(), source))?;
            }
            tar::EntryType::Regular => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|source| classify_io(parent.to_path_buf(), source))?;
                }
                let mut out = fs::File::create(&target)
                    .map_err(|source| classify_io(target.clone(), source))?;
                std::io::copy(&mut entry, &mut out)
                    .map_err(|source| classify_io(target.clone(), source))?;
                stats.files += 1;
                stats.bytes += size;
                (opts.progress)(RestoreProgress {
                    entries_done: entries_count,
                    bytes_done: stats.bytes,
                });
            }
            other => {
                return Err(CoreError::ArchiveUnsafeEntry {
                    entry: rel.to_owned(),
                    reason: format!("{other:?} entries are not server data and are never restored"),
                });
            }
        }
    }

    Ok(stats)
}

/// Validate an archive entry name and return the normalized relative path
/// (trailing directory slash stripped). Empty means "the root itself".
fn validate_name(name: &str) -> Result<&str, CoreError> {
    let bad = |entry: &str, reason: &str| {
        Err(CoreError::ArchiveUnsafeEntry {
            entry: entry.to_owned(),
            reason: reason.to_owned(),
        })
    };
    if name.is_empty() {
        return bad(name, "empty entry name");
    }
    let rel = name.strip_suffix('/').unwrap_or(name);
    if rel.starts_with('/') {
        return bad(name, "absolute paths are not allowed");
    }
    if rel.contains('\\') {
        return bad(
            name,
            "backslash separators are not allowed (use forward slashes)",
        );
    }
    if rel.contains('\0') {
        return bad(name, "NUL bytes are not allowed");
    }
    for component in rel.split('/') {
        if component.is_empty() {
            return bad(name, "empty path component");
        }
        if component == "." || component == ".." {
            return bad(
                name,
                "dot and dot-dot components are not allowed (zip-slip)",
            );
        }
        if let Some(stem) = reserved_stem(component) {
            return bad(
                name,
                &format!("the component {stem:?} is a Windows-reserved device name"),
            );
        }
    }
    Ok(rel)
}

/// `Some(component)` when a path component is a Windows-reserved device
/// name: the stem before the first dot, with trailing spaces trimmed,
/// case-insensitively (`CON`, `CON.txt`, `con .zip` all collide).
fn reserved_stem(component: &str) -> Option<String> {
    let stem = component.split('.').next().unwrap_or(component);
    let stem = stem.trim_end_matches(' ');
    if WINDOWS_RESERVED
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(stem))
    {
        Some(component.to_owned())
    } else {
        None
    }
}

/// Why a commit failed. A commit always tries to undo itself; the
/// variants say whether the undo worked.
#[derive(Debug, Clone)]
pub(crate) enum CommitError {
    /// The previous server files are back in place — nothing changed.
    RolledBack(String),
    /// The undo itself failed; the server directory needs inspection.
    RollbackBroken(String),
}

/// Commit the staged tree over the live root: move the current contents
/// aside into `rollback`, then move the staged tree in. Every step is a
/// rename on one filesystem; any failure undoes everything. This function
/// owns all the state it needs for the undo, so the rollback is complete
/// even when the failure happens mid-move-in. `pub(crate)` so tests can
/// drive the failure and rollback paths deterministically.
pub(crate) fn commit(root: &Path, staging: &Path, rollback: &Path) -> Result<(), CommitError> {
    if let Err(e) = fs::create_dir_all(rollback) {
        return Err(CommitError::RolledBack(format!(
            "cannot create the rollback dir: {e}"
        )));
    }

    let undo = |moved_aside: &[PathBuf], staged_names: &[std::ffi::OsString], reason: String| {
        match move_back(root, moved_aside, staged_names) {
            Ok(()) => {
                let _ = fs::remove_dir_all(staging);
                let _ = fs::remove_dir_all(rollback);
                CommitError::RolledBack(reason)
            }
            Err(rb) => {
                let _ = fs::remove_dir_all(staging);
                let _ = fs::remove_dir_all(rollback);
                CommitError::RollbackBroken(format!("{reason}; rollback failed: {rb}"))
            }
        }
    };

    // Move the live tree aside. The staging and rollback dirs themselves
    // stay put.
    let mut moved_aside: Vec<PathBuf> = Vec::new();
    let live: Vec<fs::DirEntry> = match fs::read_dir(root) {
        Ok(read) => read.flatten().collect(),
        Err(e) => {
            let _ = fs::remove_dir(rollback);
            return Err(CommitError::RolledBack(format!(
                "cannot list the server root: {e}"
            )));
        }
    };
    for entry in live {
        let name = entry.file_name();
        if Some(name.as_os_str()) == staging.file_name()
            || Some(name.as_os_str()) == rollback.file_name()
        {
            continue;
        }
        let from = entry.path();
        let to = rollback.join(&name);
        if let Err(e) = fs::rename(&from, &to) {
            let reason = format!("cannot move {:?} aside: {e}", from.display());
            return Err(undo(&moved_aside, &[][..], reason));
        }
        moved_aside.push(to);
    }

    // Move the staged tree in. On failure, undo: delete what came in,
    // then move the live tree back.
    let staged: Vec<fs::DirEntry> = match fs::read_dir(staging) {
        Ok(read) => read.flatten().collect(),
        Err(e) => {
            let reason = format!("cannot list the staging tree: {e}");
            return Err(undo(&moved_aside, &[][..], reason));
        }
    };
    let staged_names: Vec<std::ffi::OsString> = staged.iter().map(|e| e.file_name()).collect();
    for entry in staged {
        let from = entry.path();
        let to = root.join(entry.file_name());
        if let Err(e) = fs::rename(&from, &to) {
            let reason = format!("cannot move {:?} into place: {e}", from.display());
            return Err(undo(&moved_aside, &staged_names, reason));
        }
    }

    // Committed. The previous tree (held in the rollback dir) is deleted —
    // the backup archive itself stays on disk and can be restored again.
    let _ = fs::remove_dir_all(rollback);
    let _ = fs::remove_dir_all(staging);
    Ok(())
}

/// Undo a failed commit: delete whatever partially moved in (the staged
/// names), then move the aside-moved live entries back. Best effort — the
/// returned error is the *first* rollback failure, for the typed
/// RestoreRolledBack message.
fn move_back(
    root: &Path,
    moved_aside: &[PathBuf],
    staged_names: &[std::ffi::OsString],
) -> Result<(), String> {
    for name in staged_names {
        let path = root.join(name);
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }
    let mut first_error = None;
    for from in moved_aside {
        let file_name = from.file_name().unwrap_or_default();
        let to = root.join(file_name);
        if !from.exists() {
            continue;
        }
        if let Err(e) = fs::rename(from, &to) {
            first_error
                .get_or_insert_with(|| format!("cannot move {:?} back: {e}", from.display()));
        }
    }
    match first_error {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}
