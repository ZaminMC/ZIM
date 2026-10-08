//! The rooted server filesystem (ADR-0009): the only filesystem
//! abstraction in the project. Every operation resolves against a server
//! root and is containment-checked after canonicalization; symlinks that
//! escape the root are denied. Sync on purpose — the daemon drives these
//! through `spawn_blocking` so an interactive path never blocks.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::CoreError;

const ATOMIC_WRITE_RETRIES: usize = 3;
const ATOMIC_WRITE_RETRY_DELAY: Duration = Duration::from_millis(50);

/// Hard ceiling on one `copy` operation's total bytes, across every file
/// it lands. A synchronous, protocol-serving copy must never be able to
/// fill a disk in one call; larger trees go through backup/restore jobs.
pub const COPY_MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Depth ceiling for `copy` (a directory tree deeper than this is treated
/// as suspicious — loops via symlink are already denied) and for `search`.
pub const WALK_MAX_DEPTH: usize = 32;

/// The daemon's own staging directory: never part of a listing's answer
/// for search, never a copy source or target by that relative path.
pub const STAGING_DIR_NAME: &str = ".zamin-staging";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    /// Last modification time, Unix epoch milliseconds. None when the
    /// platform metadata could not be read.
    pub modified_ms: Option<u64>,
    /// A symlink resolving outside the root. Listed so the UI can show it;
    /// operating on it is denied.
    pub symlink_outside: bool,
}

pub struct RootedFs {
    root: PathBuf,
}

/// One `search` hit: the root-relative POSIX path plus the metadata a
/// client needs to render a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub modified_ms: Option<u64>,
}

/// `search`'s answer: sorted hits, whether the bound cut the walk short,
/// and how many entries were visited (so a client can say "searched N
/// files" honestly instead of guessing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
    pub scanned: u64,
}

/// `copy`'s answer: what landed, for the result the protocol reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyOutcome {
    pub files: u64,
    pub bytes: u64,
}

/// Running byte budget for one copy; the walk stops loudly at the cap.
struct CopyBudget {
    remaining: u64,
}

impl RootedFs {
    pub fn open(root: impl Into<PathBuf>) -> Result<RootedFs, CoreError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(CoreError::NotFound { path: root });
        }
        let canonical = fs::canonicalize(&root).map_err(|source| CoreError::Io {
            path: root.clone(),
            source,
        })?;
        Ok(RootedFs { root: canonical })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a server-root-relative POSIX-style path to a real path that
    /// is proven to stay inside the root. This is the single containment
    /// choke point; every other method goes through it.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, CoreError> {
        let rel = validate_relative(rel)?;
        let mut candidate = self.root.clone();
        for component in rel.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            candidate.push(component);
        }

        // Layer 1: canonicalize what exists and require containment.
        // Layer 2: the deepest existing ancestor must also be contained,
        // which catches symlinks sitting above a not-yet-existing leaf.
        let (existing, _remainder) = deepest_existing(&candidate);
        let anchor = fs::canonicalize(&existing).map_err(|source| CoreError::Io {
            path: existing.clone(),
            source,
        })?;
        if !anchor.starts_with(&self.root) {
            return Err(CoreError::PathEscapesRoot { path: candidate });
        }

        // Layer 3: if the target itself exists, canonicalize it too — this
        // is what exposes a symlink whose target is outside.
        if candidate.exists() {
            let canonical = fs::canonicalize(&candidate).map_err(|source| CoreError::Io {
                path: candidate.clone(),
                source,
            })?;
            if !canonical.starts_with(&self.root) {
                return Err(CoreError::PathEscapesRoot { path: candidate });
            }
            return Ok(canonical);
        }
        Ok(candidate)
    }

    pub fn read(&self, rel: &str, max_bytes: u64) -> Result<Vec<u8>, CoreError> {
        let path = self.resolve(rel)?;
        let meta = fs::metadata(&path).map_err(|source| CoreError::Io {
            path: path.clone(),
            source,
        })?;
        if !meta.is_file() {
            return Err(CoreError::NotFound { path });
        }
        if meta.len() > max_bytes {
            return Err(CoreError::ReadTooLarge {
                path,
                size: meta.len(),
                max_bytes,
            });
        }
        fs::read(&path).map_err(|source| CoreError::Io { path, source })
    }

    /// Temp file + rename, with a small retry for Windows sharing
    /// violations when another process holds the target open (ADR-0009).
    pub fn write(&self, rel: &str, bytes: &[u8]) -> Result<(), CoreError> {
        let path = self.resolve(rel)?;
        if path.is_dir() {
            return Err(CoreError::NotWritable { path });
        }
        let parent = path
            .parent()
            .ok_or_else(|| CoreError::NotWritable { path: path.clone() })?;
        fs::create_dir_all(parent).map_err(|source| CoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;

        let tmp = path.with_extension(format!("zamin-tmp-{}", std::process::id()));
        fs::write(&tmp, bytes).map_err(|source| CoreError::Io {
            path: tmp.clone(),
            source,
        })?;

        let mut attempt = 0;
        loop {
            attempt += 1;
            match fs::rename(&tmp, &path) {
                Ok(()) => return Ok(()),
                Err(source) if attempt < ATOMIC_WRITE_RETRIES && is_sharing_violation(&source) => {
                    std::thread::sleep(ATOMIC_WRITE_RETRY_DELAY);
                }
                Err(source) => {
                    let _ = fs::remove_file(&tmp);
                    return Err(CoreError::Io { path, source });
                }
            }
        }
    }

    pub fn mkdir(&self, rel: &str) -> Result<(), CoreError> {
        let path = self.resolve(rel)?;
        fs::create_dir_all(&path).map_err(|source| CoreError::Io { path, source })
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<(), CoreError> {
        let from = self.resolve(from)?;
        let to = self.resolve(to)?;
        fs::rename(&from, &to).map_err(|source| CoreError::Io { path: to, source })
    }

    /// Delete a file or an empty directory. Recursive deletion exists only
    /// as an explicit, confirmed job operation later — never here.
    pub fn delete(&self, rel: &str) -> Result<(), CoreError> {
        let path = self.resolve(rel)?;
        if path.is_dir() {
            fs::remove_dir(&path).map_err(|source| CoreError::Io { path, source })
        } else {
            fs::remove_file(&path).map_err(|source| CoreError::Io { path, source })
        }
    }

    /// Depth-1 listing; expansion is on demand (ADR-0009).
    pub fn list(&self, rel: &str) -> Result<Vec<ListingEntry>, CoreError> {
        let path = self.resolve(rel)?;
        let read = fs::read_dir(&path).map_err(|source| CoreError::Io {
            path: path.clone(),
            source,
        })?;
        let mut entries = Vec::new();
        for item in read {
            let item = item.map_err(|source| CoreError::Io {
                path: path.clone(),
                source,
            })?;
            let name = item.file_name().to_string_lossy().into_owned();
            let file_type = item.file_type().ok();
            let is_symlink = file_type.as_ref().is_some_and(|t| t.is_symlink());
            let meta = item.metadata().ok();
            let kind = if meta.as_ref().is_some_and(|m| m.is_dir()) {
                EntryKind::Dir
            } else {
                EntryKind::File
            };
            let symlink_outside = is_symlink && {
                match fs::canonicalize(item.path()) {
                    Ok(target) => !target.starts_with(&self.root),
                    Err(_) => true,
                }
            };
            let size = meta.as_ref().filter(|m| m.is_file()).map(|m| m.len());
            let modified_ms = meta.and_then(|m| {
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
            });
            entries.push(ListingEntry {
                name,
                kind,
                size,
                modified_ms,
                symlink_outside,
            });
        }
        entries.sort_by(|a, b| {
            (kind_rank(&a.kind), a.name.as_str()).cmp(&(kind_rank(&b.kind), b.name.as_str()))
        });
        Ok(entries)
    }

    /// `copy`: a file or a whole directory tree, root-contained, never
    /// overwriting an existing target. Bounded by [`COPY_MAX_TOTAL_BYTES`]
    /// so one synchronous call can never fill a disk; a tree that big
    /// belongs to the backup/restore jobs.
    ///
    /// Symlinks inside the tree are refused (typed error): a copy that
    /// silently dereferenced them would duplicate a subtree the operator
    /// did not point at, and a walk that followed them could loop. The
    /// operator's honest move is to copy the target's real name.
    pub fn copy(&self, from: &str, to: &str) -> Result<CopyOutcome, CoreError> {
        self.copy_bounded(from, to, COPY_MAX_TOTAL_BYTES)
    }

    /// `copy` with an explicit byte budget — the seam the tests use to
    /// exercise the cap without staging a two-gigabyte fixture.
    pub fn copy_bounded(
        &self,
        from: &str,
        to: &str,
        max_total_bytes: u64,
    ) -> Result<CopyOutcome, CoreError> {
        let from_path = self.resolve(from)?;
        let to_path = self.resolve(to)?;
        if to_path.exists() {
            // Copies never overwrite. The client offers a fresh name; a
            // silent replacement would destroy data a rename cannot bring
            // back.
            return Err(CoreError::CopyTargetExists { path: to_path });
        }
        // Pre-flight: measure the whole source tree before anything
        // lands. A copy that dies halfway through has already broken the
        // one promise a copy makes — nothing at the target until the
        // whole thing fits. The measure walk carries the same rules as
        // the copy walk (symlinks, depth), so every refusal surfaces
        // before the first byte moves.
        let total = measure_tree(&from_path, 0)?;
        if total > max_total_bytes {
            return Err(CoreError::CopyTooLarge {
                path: from_path,
                size: total,
                max_bytes: max_total_bytes,
            });
        }
        let mut budget = CopyBudget {
            remaining: max_total_bytes,
        };
        let mut outcome = CopyOutcome { files: 0, bytes: 0 };
        copy_recursive(&from_path, &to_path, 0, &mut budget, &mut outcome)?;
        Ok(outcome)
    }

    /// `search`: case-insensitive substring match over entry names,
    /// walking the whole root (the daemon's staging directory skipped).
    /// Bounded in depth and results; `truncated` says when the bound cut
    /// the walk short, so a client never mistakes a page for the world.
    /// The empty query matches every entry — bounded by `limit`, it is a
    /// shallow "list everything" and needs no special case.
    pub fn search(&self, query: &str, limit: usize) -> Result<SearchResult, CoreError> {
        let needle = query.to_lowercase();
        let mut hits = Vec::new();
        let mut truncated = false;
        let mut scanned: u64 = 0;
        let mut stack = vec![(self.root.clone(), String::new(), 0usize)];
        while let Some((dir, rel_prefix, depth)) = stack.pop() {
            let read = match fs::read_dir(&dir) {
                Ok(read) => read,
                // A directory that vanished mid-walk, or one the OS
                // refuses to open, shrinks the search instead of failing
                // it — the operator asked what matches, not for an
                // integrity audit.
                Err(_) => continue,
            };
            for item in read.flatten() {
                scanned += 1;
                let Ok(file_type) = item.file_type() else {
                    continue;
                };
                let name = item.file_name().to_string_lossy().into_owned();
                let rel = if rel_prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{rel_prefix}/{name}")
                };
                if file_type.is_symlink() {
                    // Skipped entirely: never descended into, never
                    // dereferenced for metadata, and not a hit — the
                    // listing shows symlinks in place; search only speaks
                    // for real files and directories (ADR-0009).
                    continue;
                }
                let is_dir = file_type.is_dir();
                if name.to_lowercase().contains(&needle) {
                    if hits.len() >= limit {
                        truncated = true;
                        break;
                    }
                    let meta = item.metadata().ok();
                    hits.push(SearchHit {
                        path: rel.clone(),
                        kind: if is_dir { EntryKind::Dir } else { EntryKind::File },
                        size: meta.as_ref().filter(|_| !is_dir).map(|m| m.len()),
                        modified_ms: meta.and_then(|m| {
                            m.modified().ok().and_then(|t| {
                                t.duration_since(std::time::UNIX_EPOCH).ok()
                            }).map(|d| d.as_millis() as u64)
                        }),
                    });
                }
                if is_dir {
                    if name == STAGING_DIR_NAME {
                        // The daemon's own staging area is never an
                        // answer to a question about the server's files.
                        continue;
                    }
                    if depth < WALK_MAX_DEPTH {
                        stack.push((dir.join(&name), rel, depth + 1));
                    } else {
                        truncated = true;
                    }
                }
            }
            if truncated && hits.len() >= limit {
                break;
            }
        }
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(SearchResult {
            hits,
            truncated,
            scanned,
        })
    }
}

/// The pre-flight walk: one metadata pass that totals the tree and
/// enforces the same symlink and depth rules the copy will. Refusals here
/// mean the copy never started, so the target stays untouched.
fn measure_tree(from: &Path, depth: usize) -> Result<u64, CoreError> {
    if depth > WALK_MAX_DEPTH {
        return Err(CoreError::CopyTooDeep {
            path: from.to_path_buf(),
        });
    }
    let meta = fs::symlink_metadata(from).map_err(|source| CoreError::Io {
        path: from.to_path_buf(),
        source,
    })?;
    if meta.is_symlink() {
        return Err(CoreError::SymlinkInCopy {
            path: from.to_path_buf(),
        });
    }
    if !meta.is_dir() {
        return Ok(meta.len());
    }
    let read = fs::read_dir(from).map_err(|source| CoreError::Io {
        path: from.to_path_buf(),
        source,
    })?;
    let mut total = 0u64;
    for item in read {
        let item = item.map_err(|source| CoreError::Io {
            path: from.to_path_buf(),
            source,
        })?;
        total += measure_tree(&item.path(), depth + 1)?;
    }
    Ok(total)
}

/// The copy walk: directories create their target and recurse, files are
/// budget-checked and streamed. The pre-flight pass has already proven
/// the tree fits; the budget here is defense in depth against a tree
/// that grew between the two passes.
fn copy_recursive(
    from: &Path,
    to: &Path,
    depth: usize,
    budget: &mut CopyBudget,
    outcome: &mut CopyOutcome,
) -> Result<(), CoreError> {
    if depth > WALK_MAX_DEPTH {
        return Err(CoreError::CopyTooDeep {
            path: from.to_path_buf(),
        });
    }
    let meta = fs::symlink_metadata(from).map_err(|source| CoreError::Io {
        path: from.to_path_buf(),
        source,
    })?;
    if meta.is_symlink() {
        return Err(CoreError::SymlinkInCopy {
            path: from.to_path_buf(),
        });
    }
    if meta.is_dir() {
        fs::create_dir_all(to).map_err(|source| CoreError::Io {
            path: to.to_path_buf(),
            source,
        })?;
        let read = fs::read_dir(from).map_err(|source| CoreError::Io {
            path: from.to_path_buf(),
            source,
        })?;
        for item in read {
            let item = item.map_err(|source| CoreError::Io {
                path: from.to_path_buf(),
                source,
            })?;
            let child_name = item.file_name();
            copy_recursive(
                &item.path(),
                &to.join(&child_name),
                depth + 1,
                budget,
                outcome,
            )?;
        }
        return Ok(());
    }
    let size = meta.len();
    if size > budget.remaining {
        return Err(CoreError::CopyTooLarge {
            path: from.to_path_buf(),
            size,
            max_bytes: budget.remaining,
        });
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|source| CoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::copy(from, to).map_err(|source| CoreError::Io {
        path: to.to_path_buf(),
        source,
    })?;
    budget.remaining -= size;
    outcome.files += 1;
    outcome.bytes += size;
    Ok(())
}

fn kind_rank(kind: &EntryKind) -> u8 {
    match kind {
        EntryKind::Dir => 0,
        EntryKind::File => 1,
    }
}

/// Atomic write for daemon-owned files (registry, per-server config).
/// Same temp+rename discipline as `RootedFs::write`, without a root.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| CoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let tmp = path.with_extension(format!("zamin-tmp-{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(|source| CoreError::Io {
        path: tmp.clone(),
        source,
    })?;
    let mut attempt = 0;
    loop {
        attempt += 1;
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(source) if attempt < ATOMIC_WRITE_RETRIES && is_sharing_violation(&source) => {
                std::thread::sleep(ATOMIC_WRITE_RETRY_DELAY);
            }
            Err(source) => {
                let _ = fs::remove_file(&tmp);
                return Err(CoreError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
}

fn validate_relative(rel: &str) -> Result<&str, CoreError> {
    if rel.is_empty() {
        return Err(CoreError::PathEscapesRoot {
            path: PathBuf::from(rel),
        });
    }
    let suspicious = rel.split('/').any(|c| c == "..")
        || rel.contains('\\')
        || rel.starts_with('/')
        || rel.contains(':')
        || rel.contains('\0');
    if suspicious {
        return Err(CoreError::PathEscapesRoot {
            path: PathBuf::from(rel),
        });
    }
    Ok(rel)
}

fn deepest_existing(path: &Path) -> (PathBuf, usize) {
    let mut current = path.to_path_buf();
    let mut depth = 0;
    loop {
        if current.exists() || current.parent().is_none() {
            return (current, depth);
        }
        match current.parent() {
            Some(parent) => {
                current = parent.to_path_buf();
                depth += 1;
            }
            None => return (current, depth),
        }
    }
}

fn is_sharing_violation(err: &io::Error) -> bool {
    // ERROR_SHARING_VIOLATION (32) surfaces as raw os error 32 on Windows.
    err.raw_os_error() == Some(32)
}

#[cfg(test)]
mod tests;
