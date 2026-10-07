//! CLI end-to-end (Phase 2 proof of done): the real `zamin` binary drives
//! the real `zamind` binary over the real protocol — register, start,
//! logs -f, attach stdin round trip, stop, JSON output, typed errors. A
//! lib-level test exercises the demultiplexing client directly.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{
    connect_with_retry, endpoint_arg, scoped_dir, spawn_daemon, wait_until, workspace_bin, Harness,
};

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

// --- remote: the CLI through the agent (ADR-0011's CLI --remote) ---

/// Owns the agent process; killed on drop.
struct TestAgent {
    child: Child,
}

impl Drop for TestAgent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The fingerprint the agent pins its TLS with, computed from the cert it
/// wrote — the same SHA-256-over-DER the agent prints at startup.
fn cert_fingerprint_hex(pem_path: &Path) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    let pem = std::fs::read_to_string(pem_path).expect("agent cert written");
    let der_b64: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(der_b64.trim())
        .expect("cert pem is valid base64");
    sha2::Sha256::digest(&der)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn cli_operates_a_remote_box_through_the_agent() {
    let harness = Harness::spawn("remote");
    let agent_dir = scoped_dir("remote-agent");

    // A port that was free a moment ago; the agent's own bind is the truth.
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe binds");
    let port = probe.local_addr().expect("local addr").port();
    drop(probe);

    let agent = Command::new(workspace_bin("zaminagent"))
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--endpoint",
            &harness.endpoint,
            "--data-dir",
            agent_dir.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("zaminagent spawns");
    let _agent = TestAgent { child: agent };

    // Bootstrap artifacts land before the listener binds.
    let token_path = agent_dir.join("token");
    let cert_path = agent_dir.join("tls").join("agent-cert.pem");
    assert!(
        wait_until(Duration::from_secs(10), || token_path.exists()
            && cert_path.exists()),
        "agent never wrote its token and cert"
    );
    let fingerprint = cert_fingerprint_hex(&cert_path);
    let token_display = token_path.to_str().unwrap().to_owned();
    let remote = format!("127.0.0.1:{port}");

    let remote_json = |args: &[&str]| {
        let mut all: Vec<String> = vec![
            "--json".into(),
            "--remote".into(),
            remote.clone(),
            "--fingerprint".into(),
            fingerprint.clone(),
            "--token-file".into(),
            token_display.clone(),
        ];
        all.extend(args.iter().map(|s| (*s).to_owned()));
        let output = Command::new(&harness.zamin)
            .args(&all)
            .output()
            .expect("zamin runs");
        assert!(
            output.status.success(),
            "zamin --remote {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("json output")
    };

    // The daemon status, seen through TLS + token + relay: the same answer
    // the local leg gives. The agent adds nothing to the conversation.
    let status = remote_json(&["daemon"]);
    assert_eq!(status["daemon"]["name"], "zamind");

    // Register and list through the remote leg.
    remote_json(&["register", "demo", harness.root.to_str().unwrap()]);
    let list = remote_json(&["list"]);
    let servers = list["servers"].as_array().expect("servers array");
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["serverId"], "demo");

    // A wrong token is a typed rejection, not a hang.
    let bad_token_dir = scoped_dir("remote-bad-token");
    let bad_token_path = bad_token_dir.join("token");
    std::fs::write(&bad_token_path, "not-the-token").expect("bad token");
    let output = Command::new(&harness.zamin)
        .args([
            "--remote",
            &remote,
            "--fingerprint",
            &fingerprint,
            "--token-file",
            bad_token_path.to_str().unwrap(),
            "--json",
            "list",
        ])
        .output()
        .expect("zamin runs");
    assert!(!output.status.success(), "a wrong token must be rejected");
    let stderr = String::from_utf8_lossy(&output.stdout);
    assert!(
        stderr.contains("AUTH_REJECTED"),
        "typed auth rejection, got: {stderr}"
    );

    // A wrong fingerprint fails the TLS handshake — the pin is the trust.
    let zeros = "0".repeat(64);
    let output = Command::new(&harness.zamin)
        .args([
            "--remote",
            &remote,
            "--fingerprint",
            &zeros,
            "--token-file",
            &token_display,
            "--json",
            "list",
        ])
        .output()
        .expect("zamin runs");
    assert!(
        !output.status.success(),
        "a wrong fingerprint must not connect"
    );
}
