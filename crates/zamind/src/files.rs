//! File-manager operations (protocol spec §8): every path is resolved
//! through the server's rooted filesystem (ADR-0009) — containment,
//! symlink denial, and typed errors are `RootedFs`'s job; this module adds
//! chunked reads, staged writes, and paged listings on top. Sync work runs
//! behind `spawn_blocking` at the engine boundary.

use std::path::Path;

use base64::Engine as _;
use zamin_protocol::error::{ErrorCode, ProtocolError};
use zamin_protocol::files::{
    EntryKind, FilesCommitResult, FilesCopyResult, FilesEntry, FilesListResult, FilesReadResult,
    FilesSearchHit, FilesSearchResult, FilesWriteResult, FILES_SEARCH_MAX_LIMIT,
};

use crate::engine::to_protocol;

const STAGING_DIR: &str = ".zamin-staging";

/// The base64 alphabet the protocol fixes (standard, padded) — one decoder
/// so a malformed payload is a typed error, not a panic.
fn decode_chunk(content: &str) -> Result<Vec<u8>, ProtocolError> {
    base64::engine::general_purpose::STANDARD
        .decode(content)
        .map_err(|e| {
            ProtocolError::new(
                ErrorCode::ProtocolInvalidRequest,
                format!("files.write content is not valid base64: {e}"),
            )
        })
}

fn encode_chunk(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// `files.list`: one page of a directory listing, directories first.
pub fn list(
    root: &Path,
    path: &str,
    offset: u32,
    limit: u32,
) -> Result<FilesListResult, ProtocolError> {
    use zamin_core::fsops::RootedFs;

    let fs = RootedFs::open(root).map_err(|e| to_protocol(&e))?;
    let normalized = if path.is_empty() { "." } else { path };
    let entries = fs.list(normalized).map_err(|e| to_protocol(&e))?;
    let total = entries.len() as u64;
    let page = entries
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|entry| FilesEntry {
            name: entry.name,
            kind: match entry.kind {
                zamin_core::fsops::EntryKind::Dir => EntryKind::Directory,
                zamin_core::fsops::EntryKind::File => EntryKind::File,
            },
            size_bytes: entry.size,
            modified_ms: entry.modified_ms,
            symlink_outside: entry.symlink_outside,
        })
        .collect();
    Ok(FilesListResult {
        path: normalized.to_owned(),
        entries: page,
        total,
    })
}

/// `files.read`: one chunk, from `offset`, at most `max_bytes`. `eof`
/// reports whether the chunk reaches the end of the file.
pub fn read(
    root: &Path,
    path: &str,
    offset: u64,
    max_bytes: u32,
) -> Result<FilesReadResult, ProtocolError> {
    use zamin_core::fsops::RootedFs;
    use zamin_protocol::files::FILES_READ_MAX_BYTES;

    let fs = RootedFs::open(root).map_err(|e| to_protocol(&e))?;
    let resolved = fs.resolve(path).map_err(|e| to_protocol(&e))?;
    let file = std::fs::File::open(&resolved).map_err(|source| {
        to_protocol(&zamin_core::error::CoreError::Io {
            path: resolved.clone(),
            source,
        })
    })?;
    let total = file
        .metadata()
        .map_err(|source| {
            to_protocol(&zamin_core::error::CoreError::Io {
                path: resolved.clone(),
                source,
            })
        })?
        .len();
    if offset > total {
        return Err(ProtocolError::new(
            ErrorCode::ProtocolInvalidRequest,
            format!("offset {offset} is past the end of {path} ({total} bytes)."),
        ));
    }

    let cap = max_bytes.clamp(1, FILES_READ_MAX_BYTES) as u64;
    let mut file = file;
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(offset)).map_err(|source| {
        to_protocol(&zamin_core::error::CoreError::Io {
            path: resolved.clone(),
            source,
        })
    })?;
    let mut buf = vec![0u8; cap.min(total - offset) as usize];
    file.read_exact(&mut buf).map_err(|source| {
        to_protocol(&zamin_core::error::CoreError::Io {
            path: resolved.clone(),
            source,
        })
    })?;

    Ok(FilesReadResult {
        data: encode_chunk(&buf),
        eof: offset + buf.len() as u64 >= total,
        total_bytes: total,
    })
}

/// `files.write`: append one decoded chunk to a staging file. The first
/// call (no handle) creates the staging file and returns its id.
pub fn write(
    root: &Path,
    staging_id: Option<&str>,
    content: &str,
) -> Result<FilesWriteResult, ProtocolError> {
    use uuid::Uuid;

    let decoded = decode_chunk(content)?;
    let staging = root.join(STAGING_DIR);
    std::fs::create_dir_all(&staging).map_err(|source| {
        to_protocol(&zamin_core::error::CoreError::Io {
            path: staging.clone(),
            source,
        })
    })?;

    let id = match staging_id {
        Some(id) if id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
            id.to_owned()
        }
        // Fresh handle, or a malformed one — a malformed id names no file
        // the daemon created, and silently reusing it would be worse than
        // a new page.
        _ => format!("stage-{}", Uuid::now_v7()),
    };
    let staged_path = staging.join(&id);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&staged_path)
        .map_err(|source| {
            to_protocol(&zamin_core::error::CoreError::Io {
                path: staged_path.clone(),
                source,
            })
        })?;
    use std::io::Write;
    file.write_all(&decoded).map_err(|source| {
        to_protocol(&zamin_core::error::CoreError::Io {
            path: staged_path.clone(),
            source,
        })
    })?;

    let bytes_staged = file.metadata().map(|m| m.len()).unwrap_or(0);
    Ok(FilesWriteResult {
        staging_id: id,
        bytes_staged,
    })
}

/// `files.commit`: one atomic rename from the staging file onto `target`,
/// then the staging directory is cleaned of this handle.
pub fn commit(
    root: &Path,
    staging_id: &str,
    target: &str,
) -> Result<FilesCommitResult, ProtocolError> {
    use zamin_core::fsops::RootedFs;

    let staging_path = root.join(STAGING_DIR).join(staging_id);
    let size = match std::fs::metadata(&staging_path) {
        Ok(meta) => meta.len(),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            // Already committed, never opened, or forged: the handle names
            // nothing — a typed miss, not an internal error.
            return Err(ProtocolError::new(
                ErrorCode::FsNotFound,
                format!("staging handle {staging_id} does not exist (already committed, or never opened)."),
            ));
        }
        Err(source) => {
            return Err(to_protocol(&zamin_core::error::CoreError::Io {
                path: staging_path,
                source,
            }))
        }
    };

    let fs = RootedFs::open(root).map_err(|e| to_protocol(&e))?;
    let resolved_target = fs.resolve(target).map_err(|e| to_protocol(&e))?;
    if resolved_target.is_dir() {
        return Err(ProtocolError::new(
            ErrorCode::FsNotWritable,
            format!("{target} is a directory; a file cannot be committed over it."),
        ));
    }
    std::fs::rename(&staging_path, &resolved_target).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            // A missing parent directory is a typed miss, not an
            // internal error — the client asked for a place the server
            // root does not have (create it with files.mkdir first).
            ProtocolError::new(
                ErrorCode::FsNotFound,
                format!(
                    "{target}'s parent directory does not exist; create it with files.mkdir first."
                ),
            )
        } else {
            to_protocol(&zamin_core::error::CoreError::Io {
                path: resolved_target.clone(),
                source,
            })
        }
    })?;
    // Best effort: the staging directory may now be empty; leave any other
    // handles alone.
    let _ = std::fs::remove_dir(root.join(STAGING_DIR));

    Ok(FilesCommitResult {
        path: target.to_owned(),
        size_bytes: size,
    })
}

/// `files.mkdir -p`, root-contained.
pub fn mkdir(root: &Path, path: &str) -> Result<(), ProtocolError> {
    use zamin_core::fsops::RootedFs;

    let fs = RootedFs::open(root).map_err(|e| to_protocol(&e))?;
    let resolved = fs.resolve(path).map_err(|e| to_protocol(&e))?;
    std::fs::create_dir_all(&resolved).map_err(|source| {
        to_protocol(&zamin_core::error::CoreError::Io {
            path: resolved,
            source,
        })
    })
}

/// `files.rename`, both ends root-contained and symlink-denied.
pub fn rename(root: &Path, from: &str, to: &str) -> Result<(), ProtocolError> {
    use zamin_core::fsops::RootedFs;

    let fs = RootedFs::open(root).map_err(|e| to_protocol(&e))?;
    fs.rename(from, to).map_err(|e| to_protocol(&e))
}

/// `files.delete`: a file, or an EMPTY directory (recursive deletion is a
/// job, not a synchronous method — spec §8).
pub fn delete(root: &Path, path: &str) -> Result<(), ProtocolError> {
    use zamin_core::fsops::RootedFs;

    let fs = RootedFs::open(root).map_err(|e| to_protocol(&e))?;
    fs.delete(path).map_err(|e| to_protocol(&e))
}

/// `files.copy`: a file or a whole tree, root-contained, never
/// overwriting. The core owns every rule (bounds, symlinks, depth); this
/// wrapper only translates the outcome into the wire shape.
pub fn copy(root: &Path, from: &str, to: &str) -> Result<FilesCopyResult, ProtocolError> {
    use zamin_core::fsops::RootedFs;

    let fs = RootedFs::open(root).map_err(|e| to_protocol(&e))?;
    let outcome = fs.copy(from, to).map_err(|e| to_protocol(&e))?;
    Ok(FilesCopyResult {
        path: to.to_owned(),
        files: outcome.files,
        bytes: outcome.bytes,
    })
}

/// `files.search`: the bounded name walk over the whole root. The query
/// is validated here (empty means the client confused search with the
/// listing) and the limit clamped to the wire cap.
pub fn search(root: &Path, query: &str, limit: u32) -> Result<FilesSearchResult, ProtocolError> {
    use zamin_core::fsops::RootedFs;

    if query.trim().is_empty() {
        return Err(ProtocolError::new(
            ErrorCode::ProtocolInvalidRequest,
            "files.search needs a query; an empty one would answer the whole server. Use files.list for a directory.",
        ));
    }
    let fs = RootedFs::open(root).map_err(|e| to_protocol(&e))?;
    let limit = limit.clamp(1, FILES_SEARCH_MAX_LIMIT);
    let result = fs
        .search(query, limit as usize)
        .map_err(|e| to_protocol(&e))?;
    Ok(FilesSearchResult {
        hits: result
            .hits
            .into_iter()
            .map(|hit| FilesSearchHit {
                path: hit.path,
                kind: match hit.kind {
                    zamin_core::fsops::EntryKind::Dir => EntryKind::Directory,
                    zamin_core::fsops::EntryKind::File => EntryKind::File,
                },
                size_bytes: hit.size,
                modified_ms: hit.modified_ms,
            })
            .collect(),
        truncated: result.truncated,
        scanned: result.scanned,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zamind-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn staged_write_commits_atomically_and_lists() {
        let root = scratch("commit");
        let handle = write(&root, None, "aGVsbG8g").unwrap(); // "hello "
        assert!(handle.staging_id.starts_with("stage-"));
        let again = write(&root, Some(&handle.staging_id), "d29ybGQ=").unwrap(); // "world"
        assert_eq!(again.bytes_staged, 11);

        commit(&root, &handle.staging_id, "greeting.txt").unwrap();
        assert_eq!(
            std::fs::read(root.join("greeting.txt")).unwrap(),
            b"hello world"
        );
        assert!(!root
            .join(".zamin-staging")
            .join(&handle.staging_id)
            .exists());

        let listing = list(&root, ".", 0, 100).unwrap();
        assert_eq!(listing.total, 1);
        assert_eq!(listing.entries[0].name, "greeting.txt");
        assert_eq!(listing.entries[0].kind, EntryKind::File);
        assert_eq!(listing.entries[0].size_bytes, Some(11));
        assert!(listing.entries[0].modified_ms.is_some());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn read_chunks_cover_the_file_and_report_eof() {
        let root = scratch("read");
        std::fs::write(root.join("data.bin"), b"0123456789").unwrap();

        let first = read(&root, "data.bin", 0, 4).unwrap();
        assert_eq!(first.data, "MDEyMw=="); // "0123"
        assert!(!first.eof);
        assert_eq!(first.total_bytes, 10);

        let last = read(&root, "data.bin", 8, 4).unwrap();
        assert_eq!(last.data, "ODk="); // "89"
        assert!(last.eof);

        let past = read(&root, "data.bin", 99, 4).unwrap_err();
        assert_eq!(past.code, ErrorCode::ProtocolInvalidRequest);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copy_lands_bytes_and_refuses_existing_targets_typed() {
        let root = scratch("copy");
        std::fs::write(root.join("a.yml"), b"alpha\n").unwrap();
        std::fs::write(root.join("b.yml"), b"beta\n").unwrap();

        let result = copy(&root, "a.yml", "backup/a.yml").unwrap();
        assert_eq!(result.path, "backup/a.yml");
        assert_eq!(result.files, 1);
        assert_eq!(result.bytes, 6);
        assert_eq!(
            std::fs::read(root.join("backup/a.yml")).unwrap(),
            b"alpha\n"
        );

        // Never overwrite: the refusal is typed and the target is intact.
        let err = copy(&root, "a.yml", "b.yml").unwrap_err();
        assert_eq!(err.code, ErrorCode::FsCopyTargetExists);
        assert_eq!(std::fs::read(root.join("b.yml")).unwrap(), b"beta\n");

        // Containment travels with copy like every other verb.
        let err = copy(&root, "a.yml", "../escape.yml").unwrap_err();
        assert_eq!(err.code, ErrorCode::FsPathEscapesRoot);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn search_answers_sorted_hits_and_skips_staging() {
        let root = scratch("search");
        std::fs::create_dir_all(root.join("plugins/EssentialsX")).unwrap();
        std::fs::create_dir_all(root.join(".zamin-staging")).unwrap();
        std::fs::write(root.join("plugins/EssentialsX/config.yml"), b"").unwrap();
        std::fs::write(root.join("plugins/EssentialsX.jar"), b"").unwrap();
        std::fs::write(root.join(".zamin-staging/stage-1"), b"").unwrap();

        let result = search(&root, "ESSENTIALS", 100).unwrap();
        let paths: Vec<&str> = result.hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["plugins/EssentialsX", "plugins/EssentialsX.jar"]
        );
        assert!(result.scanned > 0);
        assert!(!result.truncated);

        // The daemon's staging area is never an answer about the server's files.
        let result = search(&root, "stage-1", 100).unwrap();
        assert!(result.hits.is_empty());

        // An empty query is the listing's job, not the search's.
        let err = search(&root, "   ", 100).unwrap_err();
        assert_eq!(err.code, ErrorCode::ProtocolInvalidRequest);

        // The limit is clamped and the truncation flag is honest.
        let result = search(&root, "essentials", 1).unwrap();
        assert_eq!(result.hits.len(), 1);
        assert!(result.truncated);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn escape_attempts_are_typed_rejections() {
        let root = scratch("escape");
        let err = read(&root, "../../etc/passwd", 0, 10).unwrap_err();
        assert_eq!(err.code, ErrorCode::FsPathEscapesRoot);
        let err = mkdir(&root, "../outside").unwrap_err();
        assert_eq!(err.code, ErrorCode::FsPathEscapesRoot);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
