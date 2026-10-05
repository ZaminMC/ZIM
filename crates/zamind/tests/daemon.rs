//! End-to-end daemon test: spawn `zamind`, drive it over the real local
//! transport with the real protocol, and run a fake-mc-server through the
//! full lifecycle — register, start, run, console command, stop. This is
//! the Phase 1 acceptance harness (TESTING.md lifecycle matrix).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde_json::{json, Value};
use zamin_ipc::Connection;
use zamin_protocol::envelope::{IncomingMessage, RequestId};
use zamin_protocol::methods;
use zamin_protocol::server::ServerState;
use zamin_protocol::streams::{CoreEvent, StreamPayload};

fn fake_server_exe() -> PathBuf {
    // The zamind test binary and fake-mc-server share the workspace target
    // dir; `cargo test --workspace` (CI and the documented dev loop) builds
    // both. Failing loudly beats silently skipping.
    let zamind = PathBuf::from(env!("CARGO_BIN_EXE_zamind"));
    let exe = zamind
        .parent()
        .expect("target dir")
        .join(format!("fake-mc-server{}", std::env::consts::EXE_SUFFIX));
    assert!(
        exe.exists(),
        "fake-mc-server not built next to the test binary; run `cargo test --workspace`"
    );
    exe
}

struct TestServer {
    child: std::process::Child,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Tree kill: the daemon deliberately spawns servers that survive
        // its own death (ADR-0001), so killing the daemon alone leaks the
        // fake server and its inherited stdout pipe (which hangs the test
        // harness).
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("taskkill")
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
    let dir = std::env::temp_dir().join(format!("zamind-e2e-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

struct Client {
    connection: Connection,
    next_id: u64,
}

impl Client {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, Value> {
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
            "params": params,
        });
        self.connection
            .send(Bytes::from(serde_json::to_vec(&request).unwrap()))
            .await
            .expect("send request");
        loop {
            let frame = self
                .connection
                .recv()
                .await
                .expect("recv")
                .expect("connection open");
            let value: Value = serde_json::from_slice(&frame).unwrap();
            match IncomingMessage::parse(&value) {
                Some(IncomingMessage::Response(response)) => {
                    assert_eq!(response.id, RequestId::Number(self.next_id));
                    return match (response.result, response.error) {
                        (Some(result), _) => Ok(result),
                        (None, Some(error)) => Err(serde_json::to_value(error).unwrap()),
                        (None, None) => panic!("response without result or error"),
                    };
                }
                Some(IncomingMessage::Notification(_)) => continue, // streams; ignored here
                _ => panic!("unexpected message shape: {value}"),
            }
        }
    }
}

async fn wait_for_state(
    client: &mut Client,
    want: ServerState,
    timeout: Duration,
) -> Option<Value> {
    // Read notifications until the wanted transition arrives. The caller
    // must already be subscribed — subscriptions start at "now" by design
    // (ADR-0006): no replay of events older than the subscription.
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let frame = tokio::time::timeout(timeout, client.connection.recv())
            .await
            .expect("recv within timeout")
            .expect("open")
            .expect("frame");
        let value: Value = serde_json::from_slice(&frame).unwrap();
        let Some(IncomingMessage::Notification(note)) = IncomingMessage::parse(&value) else {
            continue;
        };
        let params: zamin_protocol::streams::StreamNotification =
            serde_json::from_value(note.params.unwrap()).unwrap();
        if let StreamPayload::Event { event } = params.payload {
            if let CoreEvent::ServerStateChanged { to, .. } = event {
                if to == want {
                    return Some(serde_json::to_value(event).unwrap());
                }
            }
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
async fn full_lifecycle_over_the_wire() {
    let data_dir = scoped_dir("data");
    let server_root = scoped_dir("server");
    std::fs::write(server_root.join("eula.txt"), "eula=true\n").unwrap();
    std::fs::write(server_root.join("server.jar"), b"fake jar bytes").unwrap();
    std::fs::create_dir_all(data_dir.join("servers/test")).unwrap();

    // Per-server config: point the "java" at the fake server binary, which
    // speaks both the inspection probe and the lifecycle argv.
    let java_path = fake_server_exe();
    let java_path_str = java_path.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        data_dir.join("servers/test/config.toml"),
        format!("[settings]\njavaPath = \"{java_path_str}\"\n"),
    )
    .unwrap();

    // Start the daemon on a test endpoint.
    let endpoint = zamin_ipc::Endpoint::unique_for_test("e2e");
    let endpoint_arg = match &endpoint {
        zamin_ipc::Endpoint::WindowsPipe(name) => name.clone(),
        zamin_ipc::Endpoint::UnixSocket(path) => path.to_string_lossy().into_owned(),
    };
    let child = {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_zamind"));
            command
                .args([
                    "--data-dir",
                    data_dir.to_str().unwrap(),
                    "--endpoint",
                    &endpoint_arg,
                ])
                .process_group(0);
            command.spawn().expect("zamind spawns")
        }
        #[cfg(windows)]
        {
            std::process::Command::new(env!("CARGO_BIN_EXE_zamind"))
                .args([
                    "--data-dir",
                    data_dir.to_str().unwrap(),
                    "--endpoint",
                    &endpoint_arg,
                ])
                .spawn()
                .expect("zamind spawns")
        }
    };
    let _daemon = TestServer { child };

    // Connect (the daemon needs a moment to bind).
    let mut client = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while client.is_none() {
        if Instant::now() > deadline {
            panic!("daemon never accepted a connection");
        }
        match zamin_ipc::connect(endpoint.clone()).await {
            Ok(connection) => {
                client = Some(Client {
                    connection,
                    next_id: 0,
                })
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    let mut client = client.unwrap();

    // Handshake.
    let hello = client
        .request(
            methods::DAEMON_HELLO,
            json!({
                "protocol": 1,
                "client": {"name": "zamind-test", "version": "0.0.0"},
            }),
        )
        .await
        .expect("hello");
    assert_eq!(hello["protocol"], 1);
    assert_eq!(hello["daemon"]["name"], "zamind");

    // Register the fake server.
    let registered = client
        .request(
            methods::SERVER_REGISTER,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "test",
                "displayName": "Test Server",
                "rootPath": server_root.to_string_lossy(),
            }),
        )
        .await
        .expect("register");
    assert_eq!(registered["server"]["serverId"], "test");
    assert_eq!(registered["server"]["state"], "not-running");

    // Subscribe to lifecycle events BEFORE starting: subscriptions begin at
    // "now", so this is how a client observes the transitions.
    client
        .request(
            methods::STREAMS_SUBSCRIBE,
            json!({"stream": "events", "serverId": "test"}),
        )
        .await
        .expect("subscribe events");

    // Start: accepted as starting, becomes running via startup validation.
    let started = client
        .request(
            methods::SERVER_START,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("start accepted");
    assert_eq!(started["state"], "starting");

    let running = wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("server reaches running");
    assert_eq!(running["to"], "running");

    // Console command round trip: the reply text arrives on the logs
    // stream through the supervisor's stdin pipe.
    client
        .request(
            methods::SERVER_STDIN,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "test",
                "line": "list",
            }),
        )
        .await
        .expect("stdin accepted");

    client
        .request(
            methods::STREAMS_SUBSCRIBE,
            json!({"stream": "logs", "serverId": "test"}),
        )
        .await
        .expect("subscribe logs");
    let saw_command_reply = poll_logs(&mut client, Duration::from_secs(10), |line| {
        line.contains("There are 0 of a max of 20 players online")
    })
    .await;
    assert!(
        saw_command_reply,
        "console reply must arrive on the logs stream"
    );

    // Stop: graceful, ends stopped.
    let _ = client
        .request(
            methods::SERVER_STOP,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("stop accepted");
    wait_for_state(&mut client, ServerState::Stopped, Duration::from_secs(15))
        .await
        .expect("server reaches stopped");

    let list = client
        .request(methods::SERVER_LIST, json!({}))
        .await
        .expect("list");
    assert_eq!(list["servers"][0]["state"], "stopped");

    // Status sanity.
    let status = client
        .request(methods::DAEMON_STATUS, json!({}))
        .await
        .unwrap();
    assert_eq!(status["servers"], 1);
}

async fn poll_logs(client: &mut Client, timeout: Duration, matches: impl Fn(&str) -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let frame = match tokio::time::timeout(timeout, client.connection.recv()).await {
            Ok(Ok(Some(frame))) => frame,
            _ => return false,
        };
        let value: Value = serde_json::from_slice(&frame).unwrap();
        let Some(IncomingMessage::Notification(note)) = IncomingMessage::parse(&value) else {
            continue;
        };
        let Ok(params) = serde_json::from_value::<zamin_protocol::streams::StreamNotification>(
            note.params.unwrap(),
        ) else {
            continue;
        };
        if let StreamPayload::Logs { batch } = params.payload {
            for line in batch {
                if matches(&line.line) {
                    return true;
                }
            }
        }
    }
    false
}
