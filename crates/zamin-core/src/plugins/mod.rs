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

#[cfg(test)]
mod tests;
