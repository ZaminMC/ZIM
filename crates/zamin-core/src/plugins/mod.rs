//! The plugin catalog (ADR-0012): plugins are data — loader tables and a
//! URL-speaking client — never a trait hierarchy, for the same reasons
//! the software catalog is rows and a client. Modrinth v2 is the V1
//! source (keyless, public); a second catalog is another client behind
//! the same engine methods.

mod modrinth;

pub use modrinth::{ModrinthClient, ProjectVersion, SearchHit, VersionFile};

use std::path::{Path, PathBuf};

use sha2::Sha512;

use crate::error::CoreError;
use crate::software::{download_verified, DownloadOptions, DownloadOutcome, Verified};

/// Where plugins land for a server whose root carries a `mods` directory
/// (the Fabric/Quilt/NeoForge/Forge families), and for everything else
/// (Bukkit-family `plugins`). The directory that exists is the truth on
/// disk: the daemon does not track a registered server's software, and
/// a created-but-never-run Paper server simply gets `plugins/` created
/// on first install.
pub fn target_for_root(root: &Path) -> (&'static str, PathBuf) {
    if root.join("mods").is_dir() {
        ("mods", root.join("mods"))
    } else {
        ("plugins", root.join("plugins"))
    }
}

/// The Modrinth loader slugs each target accepts, as one OR facet group.
pub fn loaders_for_target(target: &str) -> &'static [&'static str] {
    match target {
        "mods" => &["fabric", "quilt", "forge", "neoforge"],
        _ => &["paper", "spigot", "bukkit", "purpur", "folia"],
    }
}

/// Sanitize a filename that arrived over the network (ADR-0012). The
/// publisher's name is a suggestion; the bytes that touch disk are
/// whatever this function returns.
pub fn safe_file_name(name: &str) -> Result<String, CoreError> {
    let reason = |why: &str| CoreError::ArchiveUnsafeEntry {
        entry: name.to_owned(),
        reason: why.to_owned(),
    };

    // Byte length first (255 is the common filesystem cap; a UTF-8 name
    // can pass char checks while its bytes exceed any filesystem's).
    if name.is_empty() {
        return Err(reason("the file name is empty"));
    }
    if name.len() > 255 {
        return Err(reason("the file name exceeds 255 bytes"));
    }
    if name == "." || name == ".." {
        return Err(reason("the file name is a path component"));
    }
    if name
        .bytes()
        .any(|b| b < 0x20 || b == 0x7f || b == b'/' || b == b'\\' || b == b':')
    {
        return Err(reason("the file name contains path or control bytes"));
    }

    // Windows reserved device names, per component, case-insensitively
    // (ADR-0009's rule; a plugin named `con.jar` must not survive).
    let stem = name.split('.').next().unwrap_or("");
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return Err(reason("the file name is a Windows reserved device name"));
    }

    // A trailing dot or space is invisible on Windows and a different
    // file to every other platform — strip it, never trip on it later.
    let trimmed = name.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return Err(reason("the file name is only dots and spaces"));
    }
    Ok(trimmed.to_owned())
}

/// Install one published file into the target directory: sanitized name,
/// sha512-verified, atomic (staging → fsync → rename — the shared
/// downloader's discipline). ADR-0012's update rule:
///
/// - No file by that name yet → install as today.
/// - A file exists and its sha512 matches the published digest → the
///   identical file is already installed: the install short-circuits to
///   a successful outcome without a download (an idempotent re-install).
/// - A file exists with different content and `options.replace` is off
///   → the typed [`CoreError::PluginExists`]; the operator decides.
/// - `options.replace` is on → the verified download lands over the old
///   file atomically (the old bytes survive a failed download intact).
pub fn install_file(
    target_dir: &Path,
    file: &VersionFile,
    options: &DownloadOptions,
) -> Result<DownloadOutcome, CoreError> {
    let name = safe_file_name(&file.filename)?;
    if let Some(size) = file.size {
        if size == 0 {
            return Err(CoreError::ArchiveUnsafeEntry {
                entry: file.filename.clone(),
                reason: "the published file claims zero bytes".to_owned(),
            });
        }
    }

    let dest = target_dir.join(&name);
    if dest.exists() && !options.replace {
        let existing = file_sha512(&dest)?;
        if existing == file.sha512.to_ascii_lowercase() {
            let size = std::fs::metadata(&dest)
                .map(|m| m.len())
                .map_err(|source| CoreError::Io {
                    path: dest.clone(),
                    source,
                })?;
            return Ok(DownloadOutcome {
                path: dest,
                size,
                digest: existing,
            });
        }
        return Err(CoreError::PluginExists { file: name });
    }

    download_verified(
        &file.url,
        target_dir,
        &name,
        Verified::Sha512(&file.sha512),
        options,
    )
}

/// The update rule's retire step (ADR-0012): after the new version's
/// bytes have landed and verified, remove the OLD jar — the row the
/// update verdict came from. The overwrite rule is name-keyed, and a
/// version bump usually changes the published name, so an update that
/// only installs leaves both jars on disk — two versions of one plugin,
/// which a real server refuses to load.
///
/// Idempotent: a file that is already gone is a successful no-op
/// (`Ok(false)`) — the operator may have removed it while the job ran.
/// The name sanitizes like every wire name, and the deletion goes
/// through the rooted filesystem, so a symlink that leaves the server
/// root is refused exactly like `plugins.delete` refuses it.
pub fn retire_installed_file(target_dir: &Path, file_name: &str) -> Result<bool, CoreError> {
    use crate::fsops::RootedFs;
    let name = safe_file_name(file_name)?;
    let fs = RootedFs::open(target_dir)?;
    let path = fs.resolve(&name)?;
    if !path.exists() {
        return Ok(false);
    }
    fs.delete(&name)?;
    Ok(true)
}

/// Whether the file at `path` carries exactly the published digest —
/// the update rule's idempotence check (a re-install of identical bytes
/// is always allowed). A read failure is the honest error, never `false`.
pub fn file_sha512_matches(path: &Path, expected_hex: &str) -> Result<bool, CoreError> {
    Ok(file_sha512(path)? == expected_hex.to_ascii_lowercase())
}

/// The lowercase hex sha512 of a file already on disk, streamed in
/// chunks — install sizes stay small, but the discipline costs nothing.
fn file_sha512(path: &Path) -> Result<String, CoreError> {
    use sha2::Digest as _;
    let mut file = std::fs::File::open(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha512::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = std::io::Read::read(&mut file, &mut buf).map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The update check's verdict on one jar (ADR-0012's update rule, read
/// side): the disk's bytes identify the installed version — the same
/// no-shadow-state rule the install path enforces — and the catalog is
/// asked, fresh, what it now publishes for that plugin's project.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    /// The file's digest matches the newest installable version's
    /// published digest.
    UpToDate,
    /// A newer (or different-loader) installable version exists; the
    /// entry carries what `plugins.install` needs to apply it.
    UpdateAvailable,
    /// The operator action is "none through the panel", for either of
    /// two honest reasons: the catalog has no file with these bytes (a
    /// jar dropped in by hand), or it knows the bytes but publishes
    /// nothing installable for this server's loader family. Said
    /// plainly instead of inventing a version to click.
    Unmanaged,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateEntry {
    pub file_name: String,
    pub status: UpdateStatus,
    /// Present when the catalog recognized the file's bytes.
    pub project_id: Option<String>,
    /// The installed version's display number, when known.
    pub installed_version: Option<String>,
    /// The catalog's newest installable version's display number.
    pub latest_version: Option<String>,
    /// The pin that applies the update: `plugins.install`'s `versionId`.
    pub latest_version_id: Option<String>,
}

/// Check every jar in `target_dir` against the catalog. Network cost is
/// two catalog round trips per recognized file (version-from-hash, then
/// the project's version list); jars are few, so this stays a direct
/// request — the same trade `plugins.search` already makes. The report
/// sorts by file name so clients render a stable order.
pub fn check_updates(
    target_dir: &Path,
    client: &ModrinthClient,
    loaders: &[&str],
) -> Result<Vec<UpdateEntry>, CoreError> {
    let mut jars: Vec<PathBuf> = Vec::new();
    if target_dir.is_dir() {
        for entry in std::fs::read_dir(target_dir).map_err(|source| CoreError::Io {
            path: target_dir.to_path_buf(),
            source,
        })? {
            let entry = entry.map_err(|source| CoreError::Io {
                path: target_dir.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("jar") {
                jars.push(path);
            }
        }
    }
    jars.sort();

    let mut entries = Vec::with_capacity(jars.len());
    for path in jars {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        let digest = file_sha512(&path)?;
        let entry = match client.version_from_sha512(&digest)? {
            // Bytes the catalog never published: unmanaged, no story.
            None => UpdateEntry {
                file_name,
                status: UpdateStatus::Unmanaged,
                project_id: None,
                installed_version: None,
                latest_version: None,
                latest_version_id: None,
            },
            Some(installed) => {
                // The same resolve rule the install path uses: newest
                // first as returned, first version installable for this
                // loader family wins, only versions with a publishable
                // file count.
                let versions = client.versions(&installed.project_id)?;
                let latest = versions
                    .into_iter()
                    .filter(|v| v.loaders.iter().any(|l| loaders.contains(&l.as_str())))
                    .find_map(|mut v| {
                        let file = v.file.take().filter(|f| !f.sha512.is_empty())?;
                        Some((v, file))
                    });
                match latest {
                    // Known bytes, but nothing installable for this
                    // server's loader family: unmanaged, told honestly.
                    None => UpdateEntry {
                        file_name,
                        status: UpdateStatus::Unmanaged,
                        project_id: Some(installed.project_id),
                        installed_version: Some(installed.version_number),
                        latest_version: None,
                        latest_version_id: None,
                    },
                    Some((latest_version, latest_file)) => {
                        let status = if latest_file.sha512.to_ascii_lowercase() == digest {
                            UpdateStatus::UpToDate
                        } else {
                            UpdateStatus::UpdateAvailable
                        };
                        UpdateEntry {
                            file_name,
                            status,
                            project_id: Some(installed.project_id),
                            installed_version: Some(installed.version_number),
                            latest_version: Some(latest_version.version_number),
                            latest_version_id: Some(latest_version.id),
                        }
                    }
                }
            }
        };
        entries.push(entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests;
