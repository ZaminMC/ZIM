//! Shared harness for daemon integration tests: spawns the real `zamind`
//! binary, connects a protocol client over the real local transport, and
//! drives servers built on the fake-mc-server binary (TESTING.md lifecycle
//! matrix).

#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
            // Kill the daemon and ONLY the daemon. `/T` would walk into the
            // servers the daemon spawned — but on this platform each server
            // lives in its own job object without kill-on-close (ADR-0005),
            // so it must survive the harness drop exactly as it survives a
            // real daemon crash: adoption is the test subject. Tests that
            // start servers stop them (or adopt them); a leaked fake server
            // holds only an ephemeral port and dies with the runner.
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/F"])
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
    /// Event-stream notifications that arrived while a request's response
    /// was being awaited. Notifications and responses share one
    /// connection, and the daemon publishes a state transition in the
    /// same actor step that answers the request — so an event can legally
    /// race the very reply it caused (the kill path did exactly that).
    /// They are queued here in arrival order and replayed by
    /// `recv_notification`, never dropped.
    inbox: VecDeque<Value>,
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
                Some(IncomingMessage::Notification(_)) => self.inbox.push_back(value),
                _ => panic!("unexpected message shape: {value}"),
            }
        }
    }

    /// Next event-stream notification: the inbox first (events that raced
    /// a response inside `request`), then the live connection. Responses
    /// have no business here — every request awaits its own reply.
    pub async fn recv_notification(&mut self) -> Value {
        if let Some(value) = self.inbox.pop_front() {
            return value;
        }
        let frame = self
            .connection
            .recv()
            .await
            .expect("recv")
            .expect("connection open");
        let value: Value = serde_json::from_slice(&frame).unwrap();
        match IncomingMessage::parse(&value) {
            Some(IncomingMessage::Notification(_)) => value,
            Some(IncomingMessage::Response(_)) => {
                panic!("unsolicited response while awaiting a notification: {value}")
            }
            _ => panic!("unexpected message shape: {value}"),
        }
    }
}

/// Spawn the daemon binary on a test endpoint. The daemon runs in its own
/// process group (pgid == pid), so the tree kill on drop takes only the
/// daemon — servers it spawned have their own groups and survive, which is
/// exactly what adoption tests rely on.
pub fn spawn_daemon(data_dir: &Path, endpoint: &zamin_ipc::Endpoint) -> TestServer {
    spawn_daemon_with(data_dir, endpoint, &[])
}

/// `spawn_daemon` with extra CLI arguments (e.g. `--catalog-url`).
pub fn spawn_daemon_with(
    data_dir: &Path,
    endpoint: &zamin_ipc::Endpoint,
    extra_args: &[&str],
) -> TestServer {
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
                .args(extra_args)
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
                .args(extra_args)
                .spawn()
                .expect("zamind spawns")
        }
    };
    TestServer { child }
}

/// The platform-correct endpoint for a test data dir: a socket path on
/// unix; on Windows the same string rides as the pipe NAME — the spawn
/// passes it through --endpoint verbatim (from_daemon_arg makes it a
/// WindowsPipe there), so the client must dial the same name, not a
/// UnixSocket the platform layer would refuse.
pub fn endpoint_for(data_dir: &std::path::Path) -> zamin_ipc::Endpoint {
    let sock = data_dir.join("d.sock");
    #[cfg(windows)]
    {
        zamin_ipc::Endpoint::WindowsPipe(sock.to_string_lossy().into_owned())
    }
    #[cfg(unix)]
    {
        zamin_ipc::Endpoint::UnixSocket(sock)
    }
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
                    inbox: VecDeque::new(),
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
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        // Each recv is bounded by the time the whole wait has left, so the
        // deadline is honest: a wait for 10 s fails at 10 s, and the
        // caller's expect names the state that never came.
        let value = match tokio::time::timeout(remaining, client.recv_notification()).await {
            Ok(value) => value,
            Err(_) => break,
        };
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
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let value = match tokio::time::timeout(remaining, client.recv_notification()).await {
            Ok(value) => value,
            Err(_) => break,
        };
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

// --- a tiny mock HTTP server (software catalog / JDK downloads) -----------

pub struct MockHttpResponse {
    pub status: u16,
    pub reason: &'static str,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    /// When set, the body is written in 64 KiB chunks with this delay
    /// between them — deterministic mid-stream cancellation.
    pub drip_ms: u64,
}

impl MockHttpResponse {
    pub fn json(body: impl Into<Vec<u8>>) -> MockHttpResponse {
        MockHttpResponse {
            status: 200,
            reason: "OK",
            content_type: "application/json",
            body: body.into(),
            drip_ms: 0,
        }
    }
    pub fn bytes(body: Vec<u8>) -> MockHttpResponse {
        MockHttpResponse {
            status: 200,
            reason: "OK",
            content_type: "application/octet-stream",
            body,
            drip_ms: 0,
        }
    }
    pub fn not_found() -> MockHttpResponse {
        MockHttpResponse {
            status: 404,
            reason: "Not Found",
            content_type: "application/json",
            body: br#"{"error":"not found","detail":"unknown route"}"#.to_vec(),
            drip_ms: 0,
        }
    }
    pub fn dripping(mut self, drip_ms: u64) -> MockHttpResponse {
        self.drip_ms = drip_ms;
        self
    }
}

/// An in-process HTTP server speaking canned responses. Dropping it stops
/// the accept loop; tests never touch the real network.
pub struct MockHttp {
    pub url: String,
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MockHttp {
    pub fn spawn(handler: impl Fn(&str) -> MockHttpResponse + Send + Sync + 'static) -> MockHttp {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("mock binds");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        listener.set_nonblocking(true).expect("nonblocking mock");
        let handler = Arc::new(handler);
        let handle = std::thread::spawn(move || loop {
            if stop_for_thread.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                    let mut request = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        match stream.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                request.extend_from_slice(&buf[..n]);
                                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    let text = String::from_utf8_lossy(&request);
                    let path = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    let response = handler(&path);
                    let head = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response.status,
                        response.reason,
                        response.content_type,
                        response.body.len()
                    );
                    if stream.write_all(head.as_bytes()).is_err() {
                        continue;
                    }
                    if response.drip_ms == 0 {
                        let _ = stream.write_all(&response.body);
                    } else {
                        for chunk in response.body.chunks(64 * 1024) {
                            if stream.write_all(chunk).is_err() {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(response.drip_ms));
                        }
                    }
                    let _ = stream.flush();
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return,
            }
        });
        MockHttp {
            url,
            stop,
            handle: Some(handle),
        }
    }

    /// Poll `jobs.get` until the job leaves the running/queued states.
    pub async fn wait_job(
        client: &mut Client,
        job_id: &str,
        timeout: Duration,
    ) -> (String, Option<Value>) {
        let deadline = Instant::now() + timeout;
        loop {
            let job = client
                .request(methods::JOBS_GET, json!({ "jobId": job_id }))
                .await
                .expect("jobs.get");
            let state = job["state"].as_str().unwrap_or_default().to_owned();
            if state != "running" && state != "queued" {
                return (
                    state,
                    job["error"].as_object().map(|_| job["error"].clone()),
                );
            }
            assert!(Instant::now() < deadline, "job never finished: {job:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

impl Drop for MockHttp {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Accept the EULA of a freshly created server through the file surface
/// (the same move the panel makes from its typed NEEDS_EULA affordance).
pub async fn accept_eula(client: &mut Client, server_id: &str) {
    let written = client
        .request(
            methods::FILES_WRITE,
            json!({ "serverId": server_id, "content": base64("eula=true\n") }),
        )
        .await
        .expect("files.write");
    client
        .request(
            methods::FILES_COMMIT,
            json!({
                "serverId": server_id,
                "stagingId": written["stagingId"],
                "target": "eula.txt",
            }),
        )
        .await
        .expect("files.commit");
}

fn base64(text: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(text)
}
