//! Fetching a JDK (Phase 6): the Adoptium API v3 names a Temurin build
//! for this machine's OS/architecture; the downloader brings it here; a
//! safety-validated extraction unpacks it into the managed root; and the
//! found `java` is inspected like any other runtime — never trusted by
//! its directory name (ARCH-REVIEW §7).
//!
//! Extraction enforces the ADR-0009 trap list in miniature: no absolute
//! paths, no `..`, no links or devices, per-entry and total size limits,
//! and one common top-level directory (Adoptium archives always carry
//! `jdk-<release>/…`; anything else is refused, not guessed at).

use std::fs::File;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use crate::error::CoreError;
use crate::http::idempotent_get;
use crate::software::USER_AGENT;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Generous limits: a real Temurin JDK is ~200–500 MiB unpacked and a
/// few thousand entries, so the caps only ever bite on garbage.
pub const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_ENTRIES: u64 = 20_000;

pub struct AdoptiumClient {
    base: String,
    agent: ureq::Agent,
}

/// The chosen JDK build for this machine.
#[derive(Debug, Clone, PartialEq)]
pub struct JdkAsset {
    /// Release name, e.g. `jdk-21.0.12.1+1`. Becomes the directory name.
    pub release_name: String,
    pub package_name: String,
    pub url: String,
    /// sha256 of the package, fetched from the API's checksum link.
    pub sha256: String,
    /// Published size in bytes, when stated.
    pub size: Option<u64>,
}

impl AdoptiumClient {
    pub fn new(base: &str) -> AdoptiumClient {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build();
        AdoptiumClient {
            base: base.trim_end_matches('/').to_owned(),
            agent,
        }
    }

    /// The newest Temurin GA JDK for `major` on this machine.
    pub fn latest_jdk(&self, major: u32) -> Result<JdkAsset, CoreError> {
        let os = std::env::consts::OS;
        let arch = adoptium_arch();
        let url = format!(
            "{base}/assets/latest/{major}/hotspot?architecture={arch}&image_type=jdk&os={os}&vendor=eclipse",
            base = self.base,
        );
        let response = idempotent_get(|| self.agent.get(&url).call().map_err(Box::new)).map_err(
            |e| match *e {
                ureq::Error::Status(status, resp) => CoreError::Http {
                    url: url.clone(),
                    status,
                    reason: resp.status_text().to_owned(),
                },
                ureq::Error::Transport(t) => CoreError::HttpTransport {
                    url: url.clone(),
                    message: t.to_string(),
                },
            },
        )?;
        let body = response
            .into_string()
            .map_err(|e| CoreError::HttpTransport {
                url: url.clone(),
                message: e.to_string(),
            })?;
        let list: Vec<RawAsset> =
            serde_json::from_str(&body).map_err(|e| CoreError::HttpTransport {
                url,
                message: format!("unexpected Adoptium response shape: {e}"),
            })?;
        let raw = list.into_iter().next().ok_or_else(|| CoreError::Http {
            url: format!("adoptium: no JDK {major} for {os}/{arch}"),
            status: 404,
            reason: "no matching asset".to_owned(),
        })?;
        let sha256 = self.fetch_checksum(&raw.binary.package.sha256link)?;
        Ok(JdkAsset {
            release_name: raw.release_name,
            package_name: raw.binary.package.name,
            url: raw.binary.package.link,
            sha256,
            size: raw.binary.package.size,
        })
    }

    /// The checksum link answers `<sha256>  <filename>` (two spaces, like
    /// `sha256sum` output); anything else is refused.
    fn fetch_checksum(&self, link: &str) -> Result<String, CoreError> {
        let response = idempotent_get(|| self.agent.get(link).call().map_err(Box::new)).map_err(
            |e| match *e {
                ureq::Error::Status(status, resp) => CoreError::Http {
                    url: link.to_owned(),
                    status,
                    reason: resp.status_text().to_owned(),
                },
                ureq::Error::Transport(t) => CoreError::HttpTransport {
                    url: link.to_owned(),
                    message: t.to_string(),
                },
            },
        )?;
        let body = response
            .into_string()
            .map_err(|e| CoreError::HttpTransport {
                url: link.to_owned(),
                message: e.to_string(),
            })?;
        let token = body
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next())
            .ok_or_else(|| CoreError::HttpTransport {
                url: link.to_owned(),
                message: "checksum file is empty".to_owned(),
            })?;
        let token = token.to_ascii_lowercase();
        if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(CoreError::HttpTransport {
                url: link.to_owned(),
                message: format!("checksum file does not start with a sha256: {token:?}"),
            });
        }
        Ok(token)
    }
}

fn adoptium_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        "x86" => "x86",
        "riscv64" => "riscv64",
        other => other,
    }
}

#[derive(Deserialize)]
struct RawAsset {
    release_name: String,
    binary: RawBinary,
}

#[derive(Deserialize)]
struct RawBinary {
    package: RawPackage,
}

#[derive(Deserialize)]
struct RawPackage {
    name: String,
    link: String,
    #[serde(default)]
    size: Option<u64>,
    sha256link: String,
}

/// What a completed install knows about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallOutcome {
    /// The `java` executable, inspected (its major/vendor are real).
    pub java_path: PathBuf,
    pub major: u32,
    pub version_string: String,
    pub vendor: String,
    /// The runtime directory (`<managed>/<release>`).
    pub runtime_dir: PathBuf,
    pub already_installed: bool,
}

/// Download (if needed), verify, extract, inspect: the full JDK install
/// into `managed_root`. Blocking; run inside `spawn_blocking`.
pub fn install_jdk(
    managed_root: &Path,
    asset: &JdkAsset,
    cancel: Arc<AtomicBool>,
    progress: Arc<dyn Fn(InstallProgress) + Send + Sync>,
    cache: Option<&crate::cache::Cache>,
) -> Result<InstallOutcome, CoreError> {
    let runtime_dir = managed_root.join(validate_release_name(&asset.release_name)?);
    let existing = crate::java::managed_candidates(managed_root)
        .into_iter()
        .find(|bin| bin.starts_with(&runtime_dir));
    if let Some(java_path) = existing {
        // Idempotent: the same release is already on disk. Inspect it —
        // the directory name proves nothing.
        let info = crate::java::inspect(&java_path)?;
        return Ok(InstallOutcome {
            java_path: info.path,
            major: info.major,
            version_string: info.version_string,
            vendor: info.vendor,
            runtime_dir,
            already_installed: true,
        });
    }

    std::fs::create_dir_all(managed_root).map_err(|source| CoreError::Io {
        path: managed_root.to_path_buf(),
        source,
    })?;

    // The archive: cache-first (a hit is re-hashed before use), download
    // on miss, and the cached copy SURVIVES the install — the next
    // install of the same JDK works offline.
    let options = crate::software::DownloadOptions {
        cancel: Arc::clone(&cancel),
        progress: {
            let progress = Arc::clone(&progress);
            Some(Arc::new(move |p: crate::software::DownloadProgress| {
                progress(InstallProgress::Download {
                    bytes_done: p.bytes_done,
                    total: p.total,
                });
            }))
        },
        // The JDK fetch keeps the downloader's original discipline: a
        // fresh runtime directory, never an overwrite.
        replace: false,
    };
    let (archive_path, from_cache) = match cache {
        Some(cache) => {
            let artifact = cache.fetch(
                &asset.url,
                Some(crate::software::Verified::Sha256(&asset.sha256)),
                &format!(
                    "jdk-{}.archive",
                    validate_release_name(&asset.release_name)?
                ),
                &options,
            )?;
            (artifact.path, true)
        }
        None => {
            // Download next to the managed root so extraction is a plain
            // read (the no-cache path keeps its old shape).
            let archive_path =
                managed_root.join(format!(".jdk-download-{}.part", std::process::id()));
            let download = crate::software::download_to_dir(
                &asset.url,
                managed_root,
                archive_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(".jdk-download.part"),
                Some(&asset.sha256),
                &options,
            );
            let download = match download {
                Ok(d) => d,
                Err(e) => {
                    let _ = std::fs::remove_file(&archive_path);
                    return Err(e);
                }
            };
            (download.path, false)
        }
    };

    let result = extract_and_inspect(&archive_path, managed_root, &cancel, &progress);
    // The uncached archive has served its purpose either way; the cached
    // copy stays for the next install.
    if !from_cache {
        let _ = std::fs::remove_file(&archive_path);
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InstallProgress {
    Download { bytes_done: u64, total: Option<u64> },
    Extracted { entries_done: u64 },
    Inspecting,
}

fn extract_and_inspect(
    archive: &Path,
    managed_root: &Path,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<dyn Fn(InstallProgress) + Send + Sync>,
) -> Result<InstallOutcome, CoreError> {
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let runtime_dir = if name.ends_with(".zip") {
        extract_zip(archive, managed_root, cancel, progress)?
    } else {
        extract_tar_gz(archive, managed_root, cancel, progress)?
    };

    let java_path = runtime_dir
        .join("bin")
        .join(crate::platform::java_exe_name());
    if !java_path.is_file() {
        return Err(CoreError::JavaInspectFailed {
            path: runtime_dir,
            reason: "extracted archive has no bin/java".to_owned(),
        });
    }
    progress(InstallProgress::Inspecting);
    if cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_dir_all(&runtime_dir);
        return Err(CoreError::Cancelled);
    }
    let info = crate::java::inspect(&java_path).inspect_err(|_| {
        // A runtime that cannot be inspected is worthless; do not leave
        // it behind to poison discovery.
        let _ = std::fs::remove_dir_all(&runtime_dir);
    })?;
    Ok(InstallOutcome {
        java_path: info.path,
        major: info.major,
        version_string: info.version_string,
        vendor: info.vendor,
        runtime_dir,
        already_installed: false,
    })
}

/// Adoptium release names (`jdk-21.0.12.1+1`) are directory names; a
/// hostile or drifted field must never navigate anywhere.
fn validate_release_name(name: &str) -> Result<&str, CoreError> {
    let suspicious = name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.starts_with('.')
        || name.contains('\0')
        || name.len() > 100;
    if suspicious {
        return Err(CoreError::ArchiveUnsafeEntry {
            entry: name.to_owned(),
            reason: "release name is not a safe directory name".to_owned(),
        });
    }
    Ok(name)
}

/// All entries must live under one common top-level directory.
fn common_top<'a>(entries: impl Iterator<Item = &'a str>) -> Result<String, CoreError> {
    let mut top: Option<String> = None;
    for name in entries {
        let first = name.split('/').next().unwrap_or_default();
        if first.is_empty() {
            continue;
        }
        match &top {
            None => top = Some(first.to_owned()),
            Some(t) if t != first => {
                return Err(CoreError::ArchiveUnsafeEntry {
                    entry: name.to_owned(),
                    reason: "archive members do not share one top-level directory".to_owned(),
                });
            }
            _ => {}
        }
    }
    top.ok_or_else(|| CoreError::ArchiveUnsafeEntry {
        entry: "<empty>".to_owned(),
        reason: "archive holds no files".to_owned(),
    })
}

fn unsafe_entry(entry: &str, reason: &str) -> CoreError {
    CoreError::ArchiveUnsafeEntry {
        entry: entry.to_owned(),
        reason: reason.to_owned(),
    }
}

fn check_member_name(name: &str) -> Result<(), CoreError> {
    let bad = name.starts_with('/')
        || name.contains('\\')
        || name.split('/').any(|c| c == "..")
        || name.contains('\0');
    if bad {
        return Err(unsafe_entry(name, "path is not a safe relative path"));
    }
    Ok(())
}

fn extract_tar_gz(
    archive: &Path,
    managed_root: &Path,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<dyn Fn(InstallProgress) + Send + Sync>,
) -> Result<PathBuf, CoreError> {
    let file = File::open(archive).map_err(|source| CoreError::Io {
        path: archive.to_path_buf(),
        source,
    })?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    tar.set_preserve_permissions(true);

    let mut entries = tar
        .entries()
        .map_err(|e| unsafe_entry("<tar>", &e.to_string()))?;
    let mut total_bytes: u64 = 0;
    let mut entries_done: u64 = 0;
    // First pass needs the top dir before anything lands; buffer nothing
    // — validate incrementally and unpack in the same loop only after the
    // first member fixed the top directory.
    let mut top: Option<String> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            if let Some(t) = &top {
                let _ = std::fs::remove_dir_all(managed_root.join(t));
            }
            return Err(CoreError::Cancelled);
        }
        let Some(entry) = entries.next() else { break };
        let mut entry = entry.map_err(|e| unsafe_entry("<tar>", &e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| unsafe_entry("<tar>", &e.to_string()))?
            .to_string_lossy()
            .into_owned();
        check_member_name(&path)?;
        let first = path.split('/').next().unwrap_or_default().to_owned();
        if first.is_empty() {
            continue;
        }
        match &top {
            None => top = Some(first),
            Some(t) if t != &first => {
                return Err(unsafe_entry(
                    &path,
                    "archive members do not share one top-level directory",
                ));
            }
            _ => {}
        }
        let kind = entry.header().entry_type();
        // Whitelist: regular files and directories only. Links, devices,
        // fifos and every exotic entry type are refused outright.
        if !matches!(kind, tar::EntryType::Regular | tar::EntryType::Directory) {
            return Err(unsafe_entry(
                &path,
                "only regular files and directories are unpacked",
            ));
        }
        let size = entry.header().size().unwrap_or(0);
        total_bytes += size;
        if total_bytes > MAX_UNPACKED_BYTES || entries_done > MAX_ENTRIES {
            if let Some(t) = &top {
                let _ = std::fs::remove_dir_all(managed_root.join(t));
            }
            return Err(CoreError::ArchiveTooLarge {
                found: total_bytes,
                entries: entries_done,
                max_bytes: MAX_UNPACKED_BYTES,
                max_entries: MAX_ENTRIES,
            });
        }
        if let Err(e) = entry.unpack_in(managed_root) {
            let cleanup = top.clone().unwrap_or_default();
            let _ = std::fs::remove_dir_all(managed_root.join(&cleanup));
            return Err(CoreError::Io {
                path: managed_root.join(cleanup),
                source: e,
            });
        }
        entries_done += 1;
        progress(InstallProgress::Extracted { entries_done });
    }
    let top = top.ok_or_else(|| unsafe_entry("<empty>", "archive holds no files"))?;
    Ok(managed_root.join(top))
}

fn extract_zip(
    archive: &Path,
    managed_root: &Path,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<dyn Fn(InstallProgress) + Send + Sync>,
) -> Result<PathBuf, CoreError> {
    let file = File::open(archive).map_err(|source| CoreError::Io {
        path: archive.to_path_buf(),
        source,
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| unsafe_entry("<zip>", &e.to_string()))?;

    // Validate every member before extracting one.
    let mut total_bytes: u64 = 0;
    let mut names: Vec<String> = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let file = zip
            .by_index(i)
            .map_err(|e| unsafe_entry("<zip>", &e.to_string()))?;
        names.push(file.name().to_owned());
    }
    let top = common_top(names.iter().map(String::as_str))?;
    for name in &names {
        check_member_name(name)?;
    }
    for i in 0..zip.len() {
        let file = zip
            .by_index(i)
            .map_err(|e| unsafe_entry("<zip>", &e.to_string()))?;
        if file.is_symlink() {
            return Err(unsafe_entry(file.name(), "link entries are never unpacked"));
        }
        total_bytes += file.size();
        if total_bytes > MAX_UNPACKED_BYTES || (i as u64) > MAX_ENTRIES {
            return Err(CoreError::ArchiveTooLarge {
                found: total_bytes,
                entries: i as u64,
                max_bytes: MAX_UNPACKED_BYTES,
                max_entries: MAX_ENTRIES,
            });
        }
    }
    for i in 0..zip.len() {
        if cancel.load(Ordering::Relaxed) {
            let _ = std::fs::remove_dir_all(managed_root.join(&top));
            return Err(CoreError::Cancelled);
        }
        let mut file = zip
            .by_index(i)
            .map_err(|e| unsafe_entry("<zip>", &e.to_string()))?;
        let Some(enclosed) = file.enclosed_name() else {
            let _ = std::fs::remove_dir_all(managed_root.join(&top));
            return Err(unsafe_entry(
                file.name(),
                "path is not a safe relative path",
            ));
        };
        let out_path = managed_root.join(&enclosed);
        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|source| CoreError::Io {
                path: out_path.clone(),
                source,
            })?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            let mut out = File::create(&out_path).map_err(|source| CoreError::Io {
                path: out_path.clone(),
                source,
            })?;
            std::io::copy(&mut file, &mut out).map_err(|source| CoreError::Io {
                path: out_path.clone(),
                source,
            })?;
        }
        progress(InstallProgress::Extracted {
            entries_done: i as u64 + 1,
        });
    }
    Ok(managed_root.join(top))
}

#[cfg(test)]
mod tests;
