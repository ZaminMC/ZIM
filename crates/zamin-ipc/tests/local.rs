//! Local transport integration tests: real named pipes on Windows, real
//! Unix sockets on Linux, driven through the same Connection API.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncWrite};
use zamin_ipc::{connect, Connection, Endpoint, IpcError, IpcServer};

async fn echo_client(endpoint: Endpoint, expect: &'static [u8], reply: &'static [u8]) {
    let mut conn = connect(endpoint).await.expect("client connects");
    conn.send(Bytes::from_static(expect)).await.expect("send");
    let got = conn.recv().await.expect("recv").expect("frame");
    assert_eq!(&got[..], expect);
    conn.send(Bytes::from_static(reply)).await.expect("reply");
}

async fn run_echo_roundtrip(endpoint: Endpoint) {
    let mut server = IpcServer::bind(endpoint.clone())
        .await
        .expect("server binds");

    let client_endpoint = endpoint.clone();
    let client_task = tokio::spawn(echo_client(client_endpoint, b"ping", b"unused"));

    let mut conn: Connection = server.accept().await.expect("accept");
    let first = conn.recv().await.expect("recv").expect("frame");
    assert_eq!(&first[..], b"ping");
    conn.send(Bytes::from_static(b"ping")).await.expect("send");
    let second = conn.recv().await.expect("recv").expect("frame");
    assert_eq!(&second[..], b"unused");

    client_task.await.expect("client task");
}

#[tokio::test]
async fn round_trip_over_local_transport() {
    run_echo_roundtrip(Endpoint::unique_for_test("roundtrip")).await;
}

#[tokio::test]
async fn second_bind_is_already_running() {
    let endpoint = Endpoint::unique_for_test("single-instance");
    let _server = IpcServer::bind(endpoint.clone()).await.expect("first bind");

    match IpcServer::bind(endpoint).await {
        Err(IpcError::AlreadyRunning) => {}
        Err(e) => panic!("expected AlreadyRunning, got {e}"),
        Ok(_) => panic!("second bind must fail"),
    }
}

#[tokio::test]
async fn connect_without_daemon_is_no_daemon() {
    let endpoint = Endpoint::unique_for_test("no-daemon");
    match connect(endpoint).await {
        Err(IpcError::NoDaemon) => {}
        Err(e) => panic!("expected NoDaemon, got {e}"),
        Ok(_) => panic!("connect must not succeed"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn stale_socket_is_reclaimed() {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    let endpoint = Endpoint::unique_for_test("stale");
    let path = match &endpoint {
        Endpoint::UnixSocket(p) => p.clone(),
        _ => unreachable!(),
    };
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // A regular file at the socket path: connect probe fails, bind reclaims.
    std::fs::write(&path, b"not a socket").unwrap();
    std::fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();

    let server = IpcServer::bind(endpoint).await;
    assert!(server.is_ok(), "stale file must be reclaimed");
    let _ = server;
}

#[cfg(windows)]
#[tokio::test]
async fn default_endpoint_derives_a_name() {
    let endpoint = Endpoint::default_endpoint();
    let Endpoint::WindowsPipe(name) = endpoint else {
        panic!("windows default must be a pipe");
    };
    assert!(name.starts_with("zamind-"));
    assert!(!name.contains('\\'));
}

#[cfg(unix)]
#[tokio::test]
async fn default_endpoint_derives_a_path() {
    let endpoint = Endpoint::default_endpoint();
    let Endpoint::UnixSocket(path) = endpoint else {
        panic!("unix default must be a socket path");
    };
    assert!(path.ends_with("zamind.sock"));
}

// Keep the unused import honest on both platforms.
#[allow(dead_code)]
fn _io_bounds<T: AsyncRead + AsyncWrite + Unpin + Send>() {}
