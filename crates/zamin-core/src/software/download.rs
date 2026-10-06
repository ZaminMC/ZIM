//! The downloader: stream a published file to disk with progress,
//! cancellation, and checksum verification. The write path follows the
//! project's atomic-write discipline (staging file → fsync → rename), so
//! a crash mid-download can never leave a plausible-looking jar behind —
//! the target either appears complete and verified or not at all.
//!
//! Blocking on purpose: downloads run inside `spawn_blocking`, exactly
//! like backup archive walks (the daemon's only two long file operations).

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::error::CoreError;

use super::USER_AGENT;

/// Progress granularity: report per 64 KiB chunk (same cadence as the
/// backup walker's byte accounting).
const CHUNK: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub type ProgressFn = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

#[derive(Clone)]
pub struct DownloadOptions {
    /// Cooperative cancellation, checked between chunks.
    pub cancel: Arc<AtomicBool>,
    /// Byte progress, called at chunk granularity.
    pub progress: Option<ProgressFn>,
}

impl DownloadOptions {
    pub fn new() -> DownloadOptions {
        DownloadOptions {
            cancel: Arc::new(AtomicBool::new(false)),
            progress: None,
        }
    }
}

impl Default for DownloadOptions {
    fn default() -> Self {
        DownloadOptions::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DownloadProgress {
    pub bytes_done: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadOutcome {
    /// The final, verified path.
    pub path: PathBuf,
    pub size: u64,
    /// Lowercase hex sha256 of the content.
    pub sha256: String,
}

/// Download `url` into `dir` as `file_name`, verifying the published
/// sha256 when one is known. The destination must not already exist —
/// callers create fresh servers into fresh directories, and overwriting
/// a running server's jar by accident is not a race worth having.
pub fn download_to_dir(
    url: &str,
    dir: &Path,
    file_name: &str,
    expected_sha256: Option<&str>,
    options: &DownloadOptions,
) -> Result<DownloadOutcome, CoreError> {
    std::fs::create_dir_all(dir).map_err(|source| CoreError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let dest = dir.join(file_name);
    if dest.exists() {
        return Err(CoreError::Io {
            path: dest,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "download target already exists",
            ),
        });
    }
    // The staging file lives next to the destination: same filesystem,
    // so the final rename is atomic (ADR-0009 discipline).
    let staging = dir.join(format!(".zamin-staging-{}.part", std::process::id()));

    let result = stream_to(url, &staging, expected_sha256, options);

    match result {
        Ok(outcome) => {
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::rename(&staging, &dest).map_err(|source| CoreError::Io {
                path: dest.clone(),
                source,
            })?;
            Ok(DownloadOutcome {
                path: dest,
                size: outcome.size,
                sha256: outcome.sha256,
            })
        }
        Err(error) => {
            // No partial artifacts survive a failed or cancelled download.
            let _ = std::fs::remove_file(&staging);
            Err(error)
        }
    }
}

fn stream_to(
    url: &str,
    staging: &Path,
    expected_sha256: Option<&str>,
    options: &DownloadOptions,
) -> Result<DownloadOutcome, CoreError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .user_agent(USER_AGENT)
        .build();
    let response = agent.get(url).call().map_err(|e| match e {
        ureq::Error::Status(status, resp) => CoreError::Http {
            url: url.to_owned(),
            status,
            reason: resp.status_text().to_owned(),
        },
        ureq::Error::Transport(t) => CoreError::HttpTransport {
            url: url.to_owned(),
            message: t.to_string(),
        },
    })?;
    let total: Option<u64> = response
        .header("content-length")
        .and_then(|v| v.parse().ok());

    let mut file = File::create(staging).map_err(|source| CoreError::Io {
        path: staging.to_path_buf(),
        source,
    })?;
    let mut reader = response.into_reader();
    let mut hasher = Sha256::new();
    let mut bytes_done: u64 = 0;
    let mut chunk = [0u8; CHUNK];

    loop {
        if options.cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let read = reader
            .read(&mut chunk)
            .map_err(|e| CoreError::HttpTransport {
                url: url.to_owned(),
                message: format!("download interrupted after {bytes_done} bytes: {e}"),
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
        file.write_all(&chunk[..read])
            .map_err(|source| write_error(staging, source))?;
        bytes_done += read as u64;
        if let Some(progress) = &options.progress {
            progress(DownloadProgress { bytes_done, total });
        }
    }

    file.sync_all()
        .map_err(|source| write_error(staging, source))?;
    drop(file);

    let actual = hex(&hasher.finalize());
    if let Some(expected) = expected_sha256 {
        let expected = expected.to_ascii_lowercase();
        if actual != expected {
            return Err(CoreError::ChecksumMismatch {
                path: staging.to_path_buf(),
                expected,
                actual,
            });
        }
    }

    Ok(DownloadOutcome {
        path: staging.to_path_buf(),
        size: bytes_done,
        sha256: actual,
    })
}

fn write_error(path: &Path, source: std::io::Error) -> CoreError {
    // The two disk-level failure modes users can act on get their own
    // faces; everything else is an honest io error.
    match source.raw_os_error() {
        Some(28) | Some(112) | Some(122) => CoreError::DiskFull {
            path: path.to_path_buf(),
        },
        _ => CoreError::Io {
            path: path.to_path_buf(),
            source,
        },
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
