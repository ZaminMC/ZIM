//! Shared harness for daemon integration tests: spawns the real `zamind`
//! binary, connects a protocol client over the real local transport, and
//! drives servers built on the fake-mc-server binary (TESTING.md lifecycle
//! matrix).

#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde_json::{json, Value};
use zamin_ipc::Connection;
use zamin_protocol::envelope::{IncomingMessage, RequestId};
use zamin_protocol::methods;
use zamin_protocol::server::ServerState;
use zamin_protocol::streams::{CoreEvent, StreamPayload};

/// Owns the daemon process. Tree kill on drop: the daemon deliberately
/// spawns servers that survive its own death (ADR-0001), so killing the
/// daemon alone leaks the fake server and its inherited stdout pipe (which
/// hangs the test harness).
pub struct TestServer {
    pub child: std::process::Child,
}

impl Drop for TestServer {
    fn drop(&mut self) {
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

pub fn scoped_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zamind-e2e-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

pub fn fake_server_exe() -> PathBuf {
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

/// A prepared server root: EULA accepted, jar present.
pub fn make_server_root(tag: &str) -> PathBuf {
    let root = scoped_dir(tag);
    std::fs::write(root.join("eula.txt"), "eula=true\n").unwrap();
    std::fs::write(root.join("server.jar"), b"fake jar bytes").unwrap();
    root
}

/// Per-server config.toml pointing "java" at the fake server binary, plus
/// extra `[settings]` TOML appended verbatim (camelCase keys).
pub fn write_server_config(data_dir: &Path, server_id: &str, settings_toml: &str) {
    let java_path = fake_server_exe();
    let java_path_str = java_path.to_string_lossy().replace('\\', "\\\\");
    let dir = data_dir.join("servers").join(server_id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        format!("[settings]\njavaPath = \"{java_path_str}\"\n{settings_toml}"),
    )
    .unwrap();
}

pub struct Client {
    pub connection: Connection,
    pub next_id: u64,
}

impl Client {
    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value, Value> {
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
                Some(IncomingMessage::Notification(_)) => continue, // streams
                _ => panic!("unexpected message shape: {value}"),
            }
        }
    }
}

/// Spawn the daemon binary on a test endpoint. The daemon runs in its own
/// process group (pgid == pid), so the tree kill on drop takes only the
/// daemon — servers it spawned have their own groups and survive, which is
/// exactly what adoption tests rely on.
pub fn spawn_daemon(data_dir: &Path, endpoint: &zamin_ipc::Endpoint) -> TestServer {
    let endpoint_arg = match endpoint {
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
    TestServer { child }
}

/// Connect with retry (the daemon needs a moment to bind) and say hello.
pub async fn connect_daemon(endpoint: &zamin_ipc::Endpoint) -> Client {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() > deadline {
            panic!("daemon never accepted a connection");
        }
        match zamin_ipc::connect(endpoint.clone()).await {
            Ok(connection) => {
                let mut client = Client {
                    connection,
                    next_id: 0,
                };
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
                return client;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

pub async fn register_server(client: &mut Client, server_id: &str, root: &Path) -> Value {
    client
        .request(
            methods::SERVER_REGISTER,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": server_id,
                "displayName": format!("{server_id} display"),
                "rootPath": root.to_string_lossy(),
            }),
        )
        .await
        .expect("register")
}

pub async fn subscribe_events(client: &mut Client, server_id: Option<&str>) {
    let mut params = json!({"stream": "events"});
    if let Some(id) = server_id {
        params["serverId"] = json!(id);
    }
    client
        .request(methods::STREAMS_SUBSCRIBE, params)
        .await
        .expect("subscribe events");
}

pub async fn start_server(client: &mut Client, server_id: &str) -> Result<Value, Value> {
    client
        .request(
            methods::SERVER_START,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": server_id}),
        )
        .await
}

/// Read notifications until the wanted transition arrives. The caller must
/// already be subscribed — subscriptions start at "now" by design
/// (ADR-0006): no replay of events older than the subscription.
pub async fn wait_for_state(
    client: &mut Client,
    want: ServerState,
    timeout: Duration,
) -> Option<Value> {
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
        let Ok(params) = serde_json::from_value::<zamin_protocol::streams::StreamNotification>(
            note.params.unwrap(),
        ) else {
            continue;
        };
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

pub async fn poll_logs(
    client: &mut Client,
    timeout: Duration,
    matches: impl Fn(&str) -> bool,
) -> bool {
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

/// Poll server.list until the server reports the wanted state (adoption
/// happens before the daemon binds, but polling keeps tests honest).
pub async fn wait_list_state(
    client: &mut Client,
    server_id: &str,
    want: &str,
    timeout: Duration,
) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let list = client
            .request(methods::SERVER_LIST, json!({}))
            .await
            .expect("list");
        let entry = list["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["serverId"] == server_id)
            .cloned()
            .unwrap_or_else(|| panic!("server {server_id} missing from list"));
        if entry["state"] == want {
            return entry;
        }
        assert!(
            Instant::now() < deadline,
            "server never reached {want}: {entry:?}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
