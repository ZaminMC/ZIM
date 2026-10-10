//! Ensure the daemon is running (ARCH-REVIEW §1.2: "double-clicking
//! ZIM must never show a daemon error"). The webview calls
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
    resolve_daemon_binary_in(exe.as_deref(), search_path.as_deref(), daemon_binary_name())
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
    zamin_ipc::connect(endpoint.clone()).await.is_ok()
}

/// The answer budget for the liveness probe: generous enough for a
/// daemon under a startup log flood (the session task answers a ping in
/// microseconds even then), far below the webview's own reconnect
/// patience.
const ANSWER_BUDGET: Duration = Duration::from_secs(2);

/// Does the daemon ANSWER, not merely listen? The kernel owns a
/// listener's accept queue, so a WEDGED zamind.exe — hung session loop,
/// half-dead upgrade, anything that stopped servicing — still accepts
/// every connect while never speaking a frame. The old probe read that
/// wedge as "already-running", the transport's retry died against the
/// same corpse, and the panel read "always disconnected" forever. This
/// probe speaks the protocol's own ping: connect, frame, expect ANY
/// reply within the budget. A corpse answers nothing; that is the
/// verdict the heal acts on.
async fn daemon_answers(endpoint: &Endpoint) -> bool {
    let Ok(connection) = zamin_ipc::connect(endpoint.clone()).await else {
        return false;
    };
    let (mut write, mut read) = connection.split();
    let ping = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "daemon.ping",
    })
    .to_string();
    if zamin_bridge::send_frame(&mut write, &ping).await.is_err() {
        return false;
    }
    matches!(
        tokio::time::timeout(ANSWER_BUDGET, read.recv()).await,
        Ok(Ok(Some(_)))
    )
}

/// Spawn the daemon without arguments — defaults are already correct
/// (per-user endpoint, XDG/Known-Folders data dir).
///
/// Windows: `zamind` is a console-subsystem binary; spawned by this GUI
/// process without CREATE_NO_WINDOW it would allocate a brand-new
/// console — the mysterious CMD window on every first launch. The flag
/// detaches the child from any console allocation; its stdout/stderr are
/// null anyway. This is the same mechanism the daemon itself uses when
/// it spawns server processes (zamin-core platform/windows.rs, ADR-0005),
/// applied one level up.
pub fn spawn_daemon(binary: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(binary);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW — a daemon is a background service, not a
        // terminal session.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// End every WEDGED daemon the panel could have spawned. The verb is the
/// installer's own precedent (installer-hooks.nsh's ZIM_KILL_IMAGE
/// speaks the same image kill): the daemon is the panel's per-user
/// sibling, so the image name is unambiguous on Windows. Unix walks
/// /proc for exes matching the daemon's binary NAME (a per-user daemon
/// is the only zamind this user runs) and SIGKILLs the pids. A graceful
/// ask makes no sense here — the caller is here precisely because
/// nothing behind the pipe answers anything.
fn kill_stale_daemons() {
    #[cfg(windows)]
    {
        use std::process::{Command, Stdio};
        let mut command = Command::new("taskkill");
        command
            .args(["/F", "/IM", "zamind.exe"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let _ = command.status();
    }
    #[cfg(unix)]
    {
        let name = daemon_binary_name().to_owned();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<i32>().ok())
            else {
                continue;
            };
            let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else {
                continue; // another user's, or already gone
            };
            if exe.file_name().is_some_and(|n| n == name) {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
    }
}

/// Wait for the wedged daemon's pipe to actually disappear (the kernel
/// keeps the listen alive for one scheduling tick after the kill), so
/// the fresh spawn wins the single-instance race instead of the corpse.
async fn wait_pipe_clear(endpoint: &Endpoint) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if !endpoint_ready(endpoint).await {
            return;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    // Still there: the kill lost (permissions, an unexpected owner). The
    // spawn below will lose the single-instance race loudly, which is
    // the honest failure — never a silent wedge.
}

/// Probe, heal the wedged, spawn if down, wait for an ANSWERING daemon.
/// Returns what happened, typed as a small string the webview logs
/// verbatim ("already-running" | "healed" | "spawned").
pub async fn ensure_daemon() -> Result<String, String> {
    let endpoint = Endpoint::default_endpoint();
    if daemon_answers(&endpoint).await {
        return Ok("already-running".to_owned());
    }
    let binary = resolve_daemon_binary()
        .ok_or_else(|| "the zamind daemon was not found next to the panel or on PATH".to_owned())?;
    // The pipe may still accept while nothing behind it answers — end
    // the corpse BEFORE the fresh spawn, or the single-instance race
    // loses to it (the loser exits(1) and the wait below times out).
    kill_stale_daemons();
    wait_pipe_clear(&endpoint).await;
    spawn_daemon(&binary).map_err(|error| format!("could not start the daemon: {error}"))?;
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        // The spawn verdict is the ANSWER, not the bind: the webview's
        // retry would speak into a wedge otherwise.
        if daemon_answers(&endpoint).await {
            return Ok("spawned".to_owned());
        }
        if Instant::now() >= deadline {
            return Err("the daemon did not start answering within 10 s".to_owned());
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
        let dir =
            std::env::temp_dir().join(format!("zim-host-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir builds");
        dir
    }

    #[test]
    fn sibling_of_the_panel_wins() {
        let dir = temp_dir("sibling");
        let exe = dir.join("zim");
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
        let exe = dir.join("zim");
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
