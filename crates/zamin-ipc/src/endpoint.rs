//! Endpoint resolution. ADR-0001: endpoints are per-user — Windows named
//! pipes share a machine-global namespace, so the pipe name carries a
//! per-user key; Unix sockets live under `XDG_RUNTIME_DIR` (0700).

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Where a daemon listens and clients connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// Pipe name (without the `\\.\pipe\` prefix).
    WindowsPipe(String),
    /// Socket file path.
    UnixSocket(PathBuf),
}

impl Endpoint {
    /// The per-user default endpoint for this machine's OS.
    pub fn default_endpoint() -> Endpoint {
        #[cfg(windows)]
        {
            Endpoint::WindowsPipe(format!("zamind-{:016x}", user_key()))
        }
        #[cfg(unix)]
        {
            Endpoint::UnixSocket(runtime_dir().join("zamind").join("zamind.sock"))
        }
    }

    /// An endpoint unique to one test run, so concurrent tests never contend.
    pub fn unique_for_test(tag: &str) -> Endpoint {
        let key: u64 = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::process::id().hash(&mut h);
            tag.hash(&mut h);
            h.finish()
        };
        #[cfg(windows)]
        {
            Endpoint::WindowsPipe(format!("zamind-test-{key:016x}"))
        }
        #[cfg(unix)]
        {
            let dir = std::env::temp_dir().join(format!("zamind-test-{key:016x}"));
            Endpoint::UnixSocket(dir.join("zamind.sock"))
        }
    }
}

/// Stable per-user key on Windows, derived from the profile path. Pipe names
/// are global to the machine; this keeps two users' daemons from colliding.
#[cfg(windows)]
fn user_key() -> u64 {
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    profile.hash(&mut h);
    h.finish()
}

#[cfg(unix)]
fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    // No login session (CI, cron): fall back to a per-uid temp directory.
    let uid = uid_from_proc().unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("zamind-runtime-{uid}"));
    dir
}

#[cfg(unix)]
fn uid_from_proc() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}
