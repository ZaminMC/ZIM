//! Ensure the daemon is running (ARCH-REVIEW §1.2: "double-clicking
//! ZaminPanel must never show a daemon error"). The webview calls
//! `daemon_ensure` whenever a transport start fails with a connection
//! refusal; this module probes the per-user endpoint, spawns the sibling
//! `zamind` if it is down, and waits for it to bind.
//!
//! Correctness notes:
//! - Single-instance stays the daemon's job (ADR-0001): `IpcServer::bind`
//!   answers `AlreadyRunning` and the loser exits(1). A simultaneous spawn
//!   from two panels is therefore a race we are allowed to lose.
//! - Spawning the daemon is not platform-seam work in the core sense — the
//!   host crate sits outside the workspace seam (ADR-0008 guards crates/,
//!   not this thin Tauri shell) and `std::process::Command` with argv
//!   (never a shell) satisfies §12.1's spawn rules on both platforms.
//! - Detached by construction: no kill-on-close is installed on either
//!   platform (no Job Object, no wait loop), so the daemon outlives the
//!   panel exactly as the topology requires. A watcher thread reaps the
//!   child whenever it does exit (lost single-instance race) so it never
//!   lingers as a zombie for the panel's lifetime.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zamin_ipc::Endpoint;

/// How long `daemon_ensure` waits for a freshly spawned daemon to bind.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
/// Poll cadence while waiting for the socket/pipe to appear.
const POLL_INTERVAL: Duration = Duration::from_millis(150);

/// The daemon binary name on this platform.
fn daemon_binary_name() -> &'static str {
    if cfg!(windows) {
        "zamind.exe"
    } else {
        "zamind"
    }
}

/// Resolve the daemon binary: the sibling of the running panel first
/// (installed layouts — NSIS install dir, AppImage `usr/bin`, portable
/// tree), then `PATH` (development runs against `target/debug`).
pub fn resolve_daemon_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    let search_path = std::env::var_os("PATH");
    resolve_daemon_binary_in(exe.as_deref(), search_path.as_deref(), &daemon_binary_name())
}

/// Pure core of [`resolve_daemon_binary`], testable without process state.
pub fn resolve_daemon_binary_in(
    current_exe: Option<&Path>,
    path_var: Option<&std::ffi::OsStr>,
    name: &str,
) -> Option<PathBuf> {
    if let Some(exe) = current_exe {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(name);
            if sibling.is_file() {
                return Some(sibling);
            }
        }
    }
    for dir in std::env::split_paths(path_var?) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Can a client connect right now? A successful probe is dropped
/// immediately — this says nothing about protocol state, only about the
/// endpoint's existence.
pub async fn endpoint_ready(endpoint: &Endpoint) -> bool {
    matches!(zamin_ipc::connect(endpoint.clone()).await, Ok(_))
}

/// Spawn the daemon without arguments — defaults are already correct
/// (per-user endpoint, XDG/Known-Folders data dir).
pub fn spawn_daemon(binary: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(binary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Probe, spawn if down, wait for bind. Returns what happened, typed as a
/// small string the webview logs verbatim ("already-running" | "spawned").
pub async fn ensure_daemon() -> Result<String, String> {
    let endpoint = Endpoint::default_endpoint();
    if endpoint_ready(&endpoint).await {
        return Ok("already-running".to_owned());
    }
    let binary = resolve_daemon_binary().ok_or_else(|| {
        "the zamind daemon was not found next to the panel or on PATH".to_owned()
    })?;
    spawn_daemon(&binary)
        .map_err(|error| format!("could not start the daemon: {error}"))?;
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if endpoint_ready(&endpoint).await {
            return Ok("spawned".to_owned());
        }
        if Instant::now() >= deadline {
            return Err("the daemon did not start listening within 10 s".to_owned());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "zamin-host-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir builds");
        dir
    }

    #[test]
    fn sibling_of_the_panel_wins() {
        let dir = temp_dir("sibling");
        let exe = dir.join("zamin-panel");
        let daemon = dir.join("zamind");
        fs::write(&daemon, b"fake").expect("daemon stub");
        let resolved = resolve_daemon_binary_in(Some(&exe), None, "zamind");
        assert_eq!(resolved, Some(daemon));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn path_search_follows_the_sibling() {
        let dir = temp_dir("path");
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).expect("bin dir");
        let daemon = bin.join("zamind");
        fs::write(&daemon, b"fake").expect("daemon stub");
        let path = std::env::join_paths([&bin]).expect("path joins");
        let resolved = resolve_daemon_binary_in(None, Some(path.as_os_str()), "zamind");
        assert_eq!(resolved, Some(daemon));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn nothing_found_is_none() {
        let dir = temp_dir("none");
        let exe = dir.join("zamin-panel");
        let resolved = resolve_daemon_binary_in(Some(&exe), None, "zamind");
        assert_eq!(resolved, None);
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn probe_says_ready_for_a_bound_endpoint() {
        let endpoint = Endpoint::unique_for_test("host-ready");
        let _server = zamin_ipc::IpcServer::bind(endpoint.clone())
            .await
            .expect("binds a unique test endpoint");
        assert!(endpoint_ready(&endpoint).await);
    }

    #[tokio::test]
    async fn probe_says_down_for_an_unbound_endpoint() {
        let endpoint = Endpoint::unique_for_test("host-down");
        // unique_for_test has never been bound; nothing listens there.
        assert!(!endpoint_ready(&endpoint).await);
    }
}
