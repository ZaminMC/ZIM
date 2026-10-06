//! CLI end-to-end (Phase 2 proof of done): the real `zamin` binary drives
//! the real `zamind` binary over the real protocol — register, start,
//! logs -f, attach stdin round trip, stop, JSON output, typed errors. A
//! lib-level test exercises the demultiplexing client directly.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Workspace binaries share the target dir with this test binary (which
/// lives at target/<profile>/deps/). `CARGO_BIN_EXE_` only covers a
/// package's own bins, so walk the ancestors for the rest. Failing loudly
/// beats silently skipping — run `cargo test --workspace`.
fn workspace_bin(name: &str) -> PathBuf {
    let suffix = std::env::consts::EXE_SUFFIX;
    let mut dir = std::env::current_exe().expect("current exe");
    while let Some(parent) = dir.parent() {
        let candidate = parent.join(format!("{name}{suffix}"));
        if candidate.exists() {
            return candidate;
        }
        dir = parent.to_path_buf();
    }
    panic!("{name} not found next to the test binary; run `cargo test --workspace`");
}

/// Owns the daemon process. Tree kill on drop: the daemon deliberately
/// spawns servers that survive its own death (ADR-0001), so killing the
/// daemon alone leaks the fake server and its inherited stdout pipe.
struct TestDaemon {
    child: Child,
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .creation_flags(0x0800_0000)
                .status();
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.wait();
    }
}

fn scoped_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zamin-cli-e2e-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn endpoint_arg(endpoint: &zamin_ipc::Endpoint) -> String {
    match endpoint {
        zamin_ipc::Endpoint::WindowsPipe(name) => name.clone(),
        zamin_ipc::Endpoint::UnixSocket(path) => path.to_string_lossy().into_owned(),
    }
}

/// Spawn the daemon in its own process group and prepare a fake-server
/// config so `zamin start` runs the real supervision path.
struct Harness {
    _daemon: TestDaemon,
    endpoint: String,
    root: PathBuf,
    zamin: PathBuf,
}

impl Harness {
    fn spawn(tag: &str) -> Harness {
        let data_dir = scoped_dir(&format!("{tag}-data"));
        let root = scoped_dir(&format!("{tag}-root"));
        std::fs::write(root.join("eula.txt"), "eula=true\n").expect("eula");
        std::fs::write(root.join("server.jar"), b"fake jar bytes").expect("jar");
        let config_dir = data_dir.join("servers").join("demo");
        std::fs::create_dir_all(&config_dir).expect("server runtime dir");
        let java_path = workspace_bin("fake-mc-server");
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                "[settings]\njavaPath = \"{}\"\n",
                java_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .expect("config");

        let endpoint = zamin_ipc::Endpoint::unique_for_test(tag);
        let endpoint = endpoint_arg(&endpoint);
        let child = {
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                let mut command = Command::new(workspace_bin("zamind"));
                command
                    .args([
                        "--data-dir",
                        data_dir.to_str().unwrap(),
                        "--endpoint",
                        &endpoint,
                    ])
                    .process_group(0);
                command.spawn().expect("zamind spawns")
            }
            #[cfg(windows)]
            {
                Command::new(workspace_bin("zamind"))
                    .args([
                        "--data-dir",
                        data_dir.to_str().unwrap(),
                        "--endpoint",
                        &endpoint,
                    ])
                    .spawn()
                    .expect("zamind spawns")
            }
        };
        Harness {
            _daemon: TestDaemon { child },
            endpoint,
            root,
            zamin: workspace_bin("zamin"),
        }
        .wait_ready()
    }

    /// Block until the daemon accepts a connection; the first CLI call
    /// must never race the bind.
    fn wait_ready(self) -> Harness {
        assert!(
            wait_until(Duration::from_secs(10), || {
                Command::new(&self.zamin)
                    .arg("--endpoint")
                    .arg(&self.endpoint)
                    .args(["--json", "daemon"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }),
            "daemon never became ready"
        );
        self
    }

    /// Run `zamin` with the harness endpoint; captures stdout/stderr.
    fn zamin(&self, args: &[&str]) -> std::process::Output {
        Command::new(&self.zamin)
            .arg("--endpoint")
            .arg(&self.endpoint)
            .args(args)
            .output()
            .expect("zamin runs")
    }

    /// Run `zamin` with output discarded; asserts success.
    fn zamin_quiet(&self, args: &[&str]) {
        let status = Command::new(&self.zamin)
            .arg("--endpoint")
            .arg(&self.endpoint)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("zamin runs");
        assert!(status.success(), "zamin {args:?} failed");
    }

    fn zamin_json(&self, args: &[&str]) -> serde_json::Value {
        let mut all: Vec<&str> = vec!["--json"];
        all.extend_from_slice(args);
        let output = self.zamin(&all);
        assert!(
            output.status.success(),
            "zamin {args:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        serde_json::from_slice(&output.stdout).expect("zamin prints JSON")
    }
}

fn wait_until(deadline: Duration, mut check: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + deadline;
    while Instant::now() < end {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

#[test]
fn cli_registers_and_manages_the_lifecycle() {
    let harness = Harness::spawn("lifecycle");

    // Register over the wire; the typed result comes back as JSON.
    let registered = harness.zamin_json(&[
        "register",
        "demo",
        harness.root.to_str().unwrap(),
        "--name",
        "Demo Server",
    ]);
    assert_eq!(registered["server"]["serverId"], "demo");
    assert_eq!(registered["server"]["state"], "not-running");

    // Start, then poll status until the fake server reports running.
    let started = harness.zamin_json(&["start", "demo"]);
    assert_eq!(started["state"], "starting");
    assert!(
        wait_until(Duration::from_secs(15), || {
            let status = harness.zamin_json(&["status", "demo"]);
            status["state"] == "running"
        }),
        "server never reached running"
    );

    // Stop; the accepted state arrives immediately.
    let stopped = harness.zamin_json(&["stop", "demo"]);
    assert!(stopped["state"] == "stopping" || stopped["state"] == "stopped");
    assert!(
        wait_until(Duration::from_secs(15), || {
            let status = harness.zamin_json(&["status", "demo"]);
            status["state"] == "stopped"
        }),
        "server never reached stopped"
    );

    // --json list reflects the final state.
    let list = harness.zamin_json(&["list"]);
    assert_eq!(list["servers"][0]["state"], "stopped");
    assert_eq!(list["servers"][0]["displayName"], "Demo Server");
}

#[test]
fn cli_errors_are_typed_and_json_printable() {
    let harness = Harness::spawn("errors");

    let output = harness.zamin(&["--json", "status", "ghost"]);
    assert!(!output.status.success(), "ghost status must fail");
    let error: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("error object on stdout");
    assert_eq!(error["code"], "SERVER_NOT_FOUND");

    // No daemon: a human message, non-zero exit, no panic.
    let output = Command::new(&harness.zamin)
        .arg("--endpoint")
        .arg(unique_missing_endpoint())
        .arg("list")
        .output()
        .expect("zamin runs");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("zamind is not running"),
        "actionable no-daemon message expected, got: {stderr}"
    );
}

fn unique_missing_endpoint() -> String {
    let endpoint = zamin_ipc::Endpoint::unique_for_test("no-daemon");
    endpoint_arg(&endpoint)
}

#[test]
fn cli_logs_follow_streams_live_output() {
    let harness = Harness::spawn("logs-follow");
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);
    harness.zamin(&["start", "demo"]);

    // Follow mode: the opening replay carries the boot lines; read until
    // the startup-complete signature arrives, then detach.
    let mut child = Command::new(&harness.zamin)
        .arg("--endpoint")
        .arg(&harness.endpoint)
        .args(["logs", "-f", "demo"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("zamin logs -f spawns");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut reader = BufReader::new(stdout);
    let saw_done = wait_until(Duration::from_secs(15), || {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => false,
            Ok(_) => line.contains("Done (1.234s)"),
        }
    });
    let _ = child.kill();
    let _ = child.wait();
    harness.zamin(&["stop", "demo"]);
    assert!(saw_done, "logs -f must stream the boot sequence live");
}

#[test]
fn cli_attach_round_trips_console_input() {
    let harness = Harness::spawn("attach");
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);
    harness.zamin(&["start", "demo"]);

    let mut child = Command::new(&harness.zamin)
        .arg("--endpoint")
        .arg(&harness.endpoint)
        .args(["attach", "demo"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("zamin attach spawns");
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut reader = BufReader::new(stdout);

    // The banner confirms attachment; then a console command and its
    // round-tripped reply prove the stdin pipe works end to end.
    let mut line = String::new();
    let _ = reader.read_line(&mut line);
    assert!(
        line.contains("Attached to 'demo'"),
        "banner first, got: {line}"
    );

    stdin.write_all(b"list\n").expect("write list");
    stdin.flush().expect("flush stdin");
    let saw_reply = wait_until(Duration::from_secs(10), || {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => false,
            Ok(_) => line.contains("There are 0 of a max of 20 players online"),
        }
    });

    stdin.write_all(b"/quit\n").expect("write /quit");
    stdin.flush().expect("flush stdin");
    let status = wait_until(Duration::from_secs(5), || {
        matches!(child.try_wait(), Ok(Some(_)))
    });
    let exit_ok = child.try_wait().ok().flatten().map(|s| s.success());
    let _ = child.kill();
    let _ = child.wait();
    harness.zamin(&["stop", "demo"]);

    assert!(
        saw_reply,
        "console reply must come back over the logs stream"
    );
    assert!(status, "attach exits after /quit");
    assert_eq!(exit_ok, Some(true));
}

/// The library client itself: requests and stream notifications share one
/// multiplexed connection without starving each other (demux validation).
#[tokio::test(flavor = "multi_thread")]
async fn client_library_demuxes_requests_and_notifications() {
    let data_dir = scoped_dir("libdemux-data");
    let root = scoped_dir("libdemux-root");
    std::fs::write(root.join("eula.txt"), "eula=true\n").expect("eula");
    std::fs::write(root.join("server.jar"), b"fake jar bytes").expect("jar");
    let config_dir = data_dir.join("servers").join("demo");
    std::fs::create_dir_all(&config_dir).expect("dir");
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            "[settings]\njavaPath = \"{}\"\n",
            workspace_bin("fake-mc-server")
                .to_string_lossy()
                .replace('\\', "\\\\")
        ),
    )
    .expect("config");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("lib-demux");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let client = connect_with_retry(endpoint).await;

    // Subscribe BEFORE mutating: live state-changed events must arrive
    // while plain requests keep being answered on the same connection.
    let mut logs = client
        .subscribe(
            zamin_protocol::streams::StreamKind::Events,
            Some("demo".to_owned()),
        )
        .await
        .expect("subscribe");

    let registered: serde_json::Value = client
        .request(
            zamin_protocol::methods::SERVER_REGISTER,
            serde_json::json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
                "displayName": "Demo",
                "rootPath": root.to_string_lossy(),
            }),
        )
        .await
        .expect("register");
    assert_eq!(registered["server"]["state"], "not-running");

    client
        .request(
            zamin_protocol::methods::SERVER_START,
            serde_json::json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
            }),
        )
        .await
        .expect("start");

    // Notifications and further requests interleave.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut saw_running = false;
    while Instant::now() < deadline && !saw_running {
        let note = tokio::time::timeout(Duration::from_secs(2), logs.recv()).await;
        if let Ok(Ok(notification)) = note {
            if let zamin_protocol::streams::StreamPayload::Event {
                event:
                    zamin_protocol::streams::CoreEvent::ServerStateChanged {
                        to: zamin_protocol::server::ServerState::Running,
                        ..
                    },
            } = notification.payload
            {
                saw_running = true;
            }
        }
        // A request on the same connection must never starve behind the
        // notification stream.
        let status: serde_json::Value = client
            .request(
                zamin_protocol::methods::DAEMON_STATUS,
                serde_json::json!({}),
            )
            .await
            .expect("status during event flow");
        assert!(status["servers"].is_u64());
    }

    client
        .request(
            zamin_protocol::methods::SERVER_STOP,
            serde_json::json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
            }),
        )
        .await
        .expect("stop");
    assert!(saw_running, "running event arrived through the demux");
}

fn spawn_daemon(data_dir: &Path, endpoint: &zamin_ipc::Endpoint) -> TestDaemon {
    let endpoint = endpoint_arg(endpoint);
    let child = {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let mut command = Command::new(workspace_bin("zamind"));
            command
                .args([
                    "--data-dir",
                    data_dir.to_str().unwrap(),
                    "--endpoint",
                    &endpoint,
                ])
                .process_group(0);
            command.spawn().expect("zamind spawns")
        }
        #[cfg(windows)]
        {
            Command::new(workspace_bin("zamind"))
                .args([
                    "--data-dir",
                    data_dir.to_str().unwrap(),
                    "--endpoint",
                    &endpoint,
                ])
                .spawn()
                .expect("zamind spawns")
        }
    };
    TestDaemon { child }
}

async fn connect_with_retry(endpoint: zamin_ipc::Endpoint) -> zamin_cli::Client {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() > deadline {
            panic!("daemon never accepted a connection");
        }
        match zamin_cli::Client::connect(endpoint.clone()).await {
            Ok(client) => return client,
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}
