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
    /// A symlink resolving outside the root. Listed so the UI can show it;
    /// operating on it is denied.
    pub symlink_outside: bool,
}

pub struct RootedFs {
    root: PathBuf,
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
            let size = meta.filter(|m| m.is_file()).map(|m| m.len());
            entries.push(ListingEntry {
                name,
                kind,
                size,
                symlink_outside,
            });
        }
        entries.sort_by(|a, b| {
            (kind_rank(&a.kind), a.name.as_str()).cmp(&(kind_rank(&b.kind), b.name.as_str()))
        });
        Ok(entries)
    }
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
