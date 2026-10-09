//! The per-server sandbox record and storage accountant (Part 2).
//!
//! What this module IS: the storage boundary's accounting layer — a
//! budgeted walk that measures a server directory and classifies the
//! usage (ok / warning / exceeded) — plus the policy record the
//! developer tools' inspector renders. The daemon's file APIs consult
//! the verdict before writes; the periodic sampler refreshes it and
//! raises the security events.
//!
//! What this module is NOT, stated honestly: it is not an OS filesystem
//! quota. Windows has no per-directory quota primitive, so a plugin
//! that writes through its own file handles (not the daemon's APIs)
//! can grow past the budget between samples — the sampler catches it,
//! the operator is told, and the server can be stopped; the bytes
//! already written are not retroactively prevented. The memory, CPU,
//! and process-count caps ARE OS-enforced (the spawn's Job Object,
//! platform/windows.rs); this module documents that boundary and keeps
//! the accounting honest.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// One server's enforced/accounted boundary, as the inspector renders
/// it. Every field answers "what holds this?" — nothing here is
/// decorative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxPolicy {
    /// The server's jail root: the ONLY directory the daemon's file
    /// APIs will resolve into (RootedFs's containment choke point).
    pub root: PathBuf,
    /// The tree's committed-memory cap in bytes (Windows Job Object,
    /// hard). `None` documents an unset cap.
    pub memory_bytes: Option<u64>,
    /// The tree's CPU rate cap, Windows' own 1..=10000 machine scale
    /// (hard). `None` documents an unset cap.
    pub cpu_rate: Option<u32>,
    /// The tree's simultaneous-process ceiling (Windows Job Object,
    /// hard; children inherit the job).
    pub process_limit: Option<u32>,
    /// The storage budget in bytes (accounted — sampled and enforced at
    /// the daemon's write APIs, NOT an OS quota; see the module doc).
    pub storage_bytes: Option<u64>,
}

/// The measured storage verdict against the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageVerdict {
    /// Under the warning line.
    Ok,
    /// Past the warning line (default 90% of budget), under the cap.
    Warning,
    /// At or past the cap: the daemon refuses further writes.
    Exceeded,
    /// No budget configured: accounted, not capped.
    Unbounded,
}

/// The warning line as a ratio of the budget.
pub const WARNING_RATIO: f64 = 0.9;

pub fn classify_usage(bytes: u64, budget: Option<u64>) -> StorageVerdict {
    let Some(budget) = budget else {
        return StorageVerdict::Unbounded;
    };
    if budget == 0 || bytes >= budget {
        StorageVerdict::Exceeded
    } else if (bytes as f64) >= WARNING_RATIO * budget as f64 {
        StorageVerdict::Warning
    } else {
        StorageVerdict::Ok
    }
}

/// The budgeted walk's result. The walk stops the moment the budget is
/// exceeded — measuring a 500 GB runaway against a 100 GB budget costs
/// 100 GB of traversal, never 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageWalk {
    pub bytes: u64,
    pub files: u64,
    /// True when the walk stopped because the budget was passed (the
    /// bytes figure is then a lower bound, honestly labelled by the
    /// verdict that follows it).
    pub stopped_early: bool,
}

/// Measure a directory tree against a budget. Symlinks are NOT followed
/// (a link out of the sandbox cannot smuggle another tree's bytes into
/// the count, nor can it turn the walk into a cycle). Cancellation is
/// honored between entries.
pub fn measure_dir(
    root: &Path,
    budget: u64,
    cancel: &AtomicBool,
) -> std::io::Result<StorageWalk> {
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut bytes: u64 = 0;
    let mut files: u64 = 0;
    let mut stopped_early = false;

    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "storage walk cancelled",
            ));
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            // An unreadable subdirectory (permissions, racing delete) is
            // skipped, not fatal: the walk is an accounting of what can
            // be counted, and the verdict errs on the generous side.
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            // This walk never follows links: a symlink inside the jail
            // contributes its own path, never its target's bytes.
            if meta.is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                bytes = bytes.saturating_add(meta.len());
                files += 1;
                if budget > 0 && bytes >= budget {
                    stopped_early = true;
                    break;
                }
            }
        }
        if stopped_early {
            break;
        }
    }
    Ok(StorageWalk {
        bytes,
        files,
        stopped_early,
    })
}

/// Append-only journal of the sandbox's security notices, alongside the
/// live events stream: an operator reading the journal sees the same
/// story the UI saw. One file per data dir, JSONL.
pub struct SecurityJournal {
    path: PathBuf,
    lock: Arc<std::sync::Mutex<()>>,
}

impl SecurityJournal {
    pub fn new(data_dir: &Path) -> SecurityJournal {
        SecurityJournal {
            path: data_dir.join("security.log"),
            lock: Arc::new(std::sync::Mutex::new(())),
        }
    }

    /// Record one notice: server, kind, detail. Open-per-write (a
    /// rotated or deleted file cannot wedge the daemon) with append
    /// semantics, fsync'd — a boundary event that a crash loses was
    /// never recorded.
    pub fn record(
        &self,
        server_id: Option<&str>,
        kind: &str,
        detail: &str,
    ) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "ts": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            "serverId": server_id,
            "kind": kind,
            "detail": detail,
        });
        let line = serde_json::to_string(&entry)
            .unwrap_or_else(|_| r#"{"kind":"security.journal.serialize.failed"}"#.to_owned());
        let _guard = self.lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()
    }
}

/// The read-back for the developer tools: the journal's newest `cap`
/// entries, oldest first. A missing or unreadable journal reads as
/// empty (nothing recorded yet is an honest state, not an error).
pub fn read_journal(path: &Path, cap: usize) -> Vec<serde_json::Value> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let mut entries: Vec<serde_json::Value> = String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if entries.len() > cap {
        entries.drain(..entries.len() - cap);
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zamin-sandbox-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &Path, len: usize) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, vec![0u8; len]).unwrap();
    }

    #[test]
    fn classify_maps_the_three_verdicts_and_the_unbudgeted_case() {
        assert_eq!(classify_usage(10, Some(100)), StorageVerdict::Ok);
        assert_eq!(classify_usage(95, Some(100)), StorageVerdict::Warning);
        assert_eq!(classify_usage(100, Some(100)), StorageVerdict::Exceeded);
        assert_eq!(classify_usage(500, Some(100)), StorageVerdict::Exceeded);
        assert_eq!(classify_usage(500, None), StorageVerdict::Unbounded);
        // A zero budget is a wall, not a divide-by-zero.
        assert_eq!(classify_usage(0, Some(0)), StorageVerdict::Exceeded);
    }

    #[test]
    fn measure_walks_the_tree_and_stops_at_the_budget() {
        let root = temp_root("walk");
        write_file(&root.join("a/logs/latest.log"), 300);
        write_file(&root.join("world/region/r.1.mca"), 700);
        let cancel = AtomicBool::new(false);

        let walk = measure_dir(&root, 10_000, &cancel).unwrap();
        assert_eq!(walk.bytes, 1000);
        assert_eq!(walk.files, 2);
        assert!(!walk.stopped_early);

        // A 600-byte budget stops the walk early — the bytes are a
        // lower bound by design, and the verdict that follows is
        // Exceeded.
        let walk = measure_dir(&root, 600, &cancel).unwrap();
        assert!(walk.stopped_early);
        assert!(walk.bytes >= 600);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn measure_never_follows_symlinks() {
        let root = temp_root("symlink");
        write_file(&root.join("inside/file.bin"), 500);
        let outside = temp_root("symlink-outside");
        write_file(&outside.join("huge.bin"), 100_000);
        std::os::unix::fs::symlink(&outside.join("huge.bin"), root.join("inside/link.bin"))
            .unwrap();
        let cancel = AtomicBool::new(false);
        let walk = measure_dir(&root, u64::MAX, &cancel).unwrap();
        assert_eq!(
            walk.bytes, 500,
            "a link out of the jail cannot smuggle its target's bytes in"
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn journal_records_and_reads_back_oldest_first() {
        let root = temp_root("journal");
        let journal = SecurityJournal::new(&root);
        journal
            .record(Some("survival"), "storage_limit_reached", "past 100 GiB")
            .unwrap();
        journal
            .record(None, "suspicious_resource_usage", "cpu saturated")
            .unwrap();
        let entries = read_journal(&root.join("security.log"), 100);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["kind"], "storage_limit_reached");
        assert_eq!(entries[1]["serverId"], serde_json::Value::Null);
        // The cap keeps the read bounded; the newest survive.
        let capped = read_journal(&root.join("security.log"), 1);
        assert_eq!(capped.len(), 1);
        assert_eq!(capped[0]["kind"], "suspicious_resource_usage");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn journal_read_of_a_missing_file_is_empty_not_an_error() {
        let root = temp_root("journal-missing");
        let entries = read_journal(&root.join("security.log"), 100);
        assert!(entries.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
