//! The package stage: a deterministic zip of the resolved selection with
//! a self-describing manifest embedded at `zamin-publish.json`. Built in
//! staging, fsynced, hashed, then committed by rename — a crash mid-pack
//! leaves the previous package and a stray temp file, never a half
//! package presented as current (§42's transactional semantics).
//!
//! Determinism: entries are written in the resolved map's path order
//! with a fixed timestamp, so two publishes of identical content produce
//! byte-identical archives — which is what makes the package digest a
//! meaningful identity, comparable across machines.

use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};

use crate::error::CoreError;
use crate::publish::selection::ResolvedFile;

/// The manifest's name inside every package.
pub const MANIFEST_FILE_NAME: &str = "zamin-publish.json";
/// Bump when the manifest shape ever changes.
pub const PACKAGE_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishManifestEntry {
    pub path: String,
    pub sha512: String,
    pub size: u64,
}

/// The self-describing manifest: what this package is, who made it, and
/// exactly which bytes went in. Lives inside the package; the daemon's
/// publication record carries the same digests after the provider
/// answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishManifest {
    pub format_version: u32,
    pub created_at_ms: i64,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<String>,
    pub provider_id: String,
    /// The selected files only — the manifest itself is not counted.
    pub file_count: u64,
    pub total_bytes: u64,
    pub files: Vec<PublishManifestEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageProgress {
    pub files_done: u64,
    pub total_files: u64,
    pub bytes_done: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOutcome {
    pub path: std::path::PathBuf,
    pub sha512: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub total_bytes: u64,
    pub manifest: PublishManifest,
}

pub struct PackageOptions<'a> {
    pub files: &'a [&'a ResolvedFile],
    /// Final resting place of the package (e.g. `<publish>/package.zip`).
    pub out_path: &'a Path,
    /// Staging file INSIDE the same directory as `out_path`, so the
    /// commit rename never crosses a filesystem.
    pub staging_path: &'a Path,
    pub title: String,
    pub description: String,
    pub version: Option<String>,
    pub changelog: Option<String>,
    pub provider_id: String,
    pub created_at_ms: i64,
    pub max_entries: u64,
    pub max_total_bytes: u64,
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<dyn Fn(PackageProgress) + Send + Sync>,
}

fn cancelled(opts: &PackageOptions<'_>) -> bool {
    opts.cancel.load(Ordering::Relaxed)
}

/// Build, hash, and commit the package. Every file is re-read from its
/// resolved absolute path — a file that vanished since the resolve is a
/// typed failure (the honest answer: re-run the preview, the world
/// moved).
pub fn build_package(opts: &mut PackageOptions<'_>) -> Result<PackageOutcome, CoreError> {
    if opts.files.len() as u64 > opts.max_entries {
        return Err(CoreError::PublishTooLarge {
            found: opts.files.len() as u64,
            entries: opts.files.len() as u64,
            max_bytes: opts.max_total_bytes,
            max_entries: opts.max_entries,
        });
    }
    let total_bytes: u64 = opts.files.iter().map(|f| f.size).sum();
    if total_bytes > opts.max_total_bytes {
        return Err(CoreError::PublishTooLarge {
            found: total_bytes,
            entries: opts.files.len() as u64,
            max_bytes: opts.max_total_bytes,
            max_entries: opts.max_entries,
        });
    }

    let manifest = PublishManifest {
        format_version: PACKAGE_FORMAT_VERSION,
        created_at_ms: opts.created_at_ms,
        title: opts.title.clone(),
        description: opts.description.clone(),
        version: opts.version.clone(),
        changelog: opts.changelog.clone(),
        provider_id: opts.provider_id.clone(),
        file_count: opts.files.len() as u64,
        total_bytes,
        files: opts
            .files
            .iter()
            .map(|f| PublishManifestEntry {
                path: f.path.clone(),
                sha512: f.sha512.clone(),
                size: f.size,
            })
            .collect(),
    };

    if let Some(parent) = opts.out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let staging_file =
        std::fs::File::create(opts.staging_path).map_err(|source| CoreError::Io {
            path: opts.staging_path.to_path_buf(),
            source,
        })?;
    let mut writer = zip::ZipWriter::new(staging_file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);

    write_entry(
        &mut writer,
        &options,
        MANIFEST_FILE_NAME,
        &manifest_json(&manifest)?,
    )?;

    let total_files = opts.files.len() as u64;
    let mut bytes_done = 0u64;
    for (files_done, file) in opts.files.iter().enumerate() {
        let files_done = files_done as u64 + 1;
        if cancelled(opts) {
            let _ = std::fs::remove_file(opts.staging_path);
            return Err(CoreError::Cancelled);
        }
        let mut source = std::fs::File::open(&file.abs).map_err(|source| CoreError::Io {
            path: file.abs.clone(),
            source,
        })?;
        writer
            .start_file(file.path.as_str(), options)
            .map_err(|e| CoreError::PublishStateCorrupt {
                path: opts.staging_path.to_path_buf(),
                reason: format!("zip entry {}: {e}", file.path),
            })?;
        std::io::copy(&mut source, &mut writer).map_err(|source| CoreError::Io {
            path: file.abs.clone(),
            source,
        })?;
        bytes_done += file.size;
        (opts.progress)(PackageProgress {
            files_done,
            total_files,
            bytes_done,
        });
    }
    writer
        .finish()
        .map_err(|e| CoreError::PublishStateCorrupt {
            path: opts.staging_path.to_path_buf(),
            reason: format!("finishing the archive: {e}"),
        })?;

    // fsync the staged archive before it becomes THE package.
    let mut staged = std::fs::OpenOptions::new()
        .read(true)
        .open(opts.staging_path)
        .map_err(|source| CoreError::Io {
            path: opts.staging_path.to_path_buf(),
            source,
        })?;
    staged.sync_all().map_err(|source| CoreError::Io {
        path: opts.staging_path.to_path_buf(),
        source,
    })?;
    let sha = hash_file(&mut staged)?;
    let size_bytes = staged
        .metadata()
        .map_err(|source| CoreError::Io {
            path: opts.staging_path.to_path_buf(),
            source,
        })?
        .len();
    drop(staged);

    // The commit: rename staging over the destination. Unix replaces
    // atomically; Windows refuses to replace an existing file, so there
    // the old package is removed first — a wider window, but the record
    // is what calls a package current, and it still points only at a
    // complete artifact.
    #[allow(unused_mut)]
    let mut renamed = std::fs::rename(opts.staging_path, opts.out_path);
    if renamed.is_err() && opts.out_path.exists() {
        std::fs::remove_file(opts.out_path).map_err(|source| CoreError::Io {
            path: opts.out_path.to_path_buf(),
            source,
        })?;
        renamed = std::fs::rename(opts.staging_path, opts.out_path);
    }
    renamed.map_err(|source| CoreError::Io {
        path: opts.out_path.to_path_buf(),
        source,
    })?;

    Ok(PackageOutcome {
        path: opts.out_path.to_path_buf(),
        sha512: sha,
        size_bytes,
        file_count: manifest.file_count,
        total_bytes: manifest.total_bytes,
        manifest,
    })
}

fn manifest_json(manifest: &PublishManifest) -> Result<Vec<u8>, CoreError> {
    let mut bytes =
        serde_json::to_vec_pretty(manifest).map_err(|e| CoreError::PublishStateCorrupt {
            path: std::path::PathBuf::from(MANIFEST_FILE_NAME),
            reason: e.to_string(),
        })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_entry<W: Write + Seek>(
    writer: &mut zip::ZipWriter<W>,
    options: &zip::write::SimpleFileOptions,
    name: &str,
    bytes: &[u8],
) -> Result<(), CoreError> {
    writer
        .start_file(name, *options)
        .map_err(|e| CoreError::PublishStateCorrupt {
            path: std::path::PathBuf::from(name),
            reason: e.to_string(),
        })?;
    writer.write_all(bytes).map_err(|source| CoreError::Io {
        path: std::path::PathBuf::from(name),
        source,
    })
}

fn hash_file(file: &mut std::fs::File) -> Result<String, CoreError> {
    let _ = file.seek(std::io::SeekFrom::Start(0));
    let mut hasher = Sha512::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|source| CoreError::Io {
            path: std::path::PathBuf::from("<package>"),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
