//! The remote path end to end over real sockets: TLS + token gate +
//! relay against a stub daemon, per ADR-0011. The stub answers hello and
//! echoes every later request — the agent must add nothing to the
//! conversation.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::net::TcpStream;
use zamin_agent::client::{self, RemoteConnect, Trust};
use zamin_agent::server::AgentServer;
use zamin_agent::tls::TlsMaterial;
use zamin_ipc::{Endpoint, IpcServer};
use zamin_protocol::envelope::{IncomingMessage, Request, RequestId, Response};
use zamin_protocol::error::ErrorCode;
use zamin_protocol::handshake::{ClientInfo, HelloParams};
use zamin_protocol::methods::DAEMON_HELLO;

const TOKEN: &str = "test-token-value";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zamin-agent-remote-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn hello_frame(auth: Option<&str>, id: u64) -> Bytes {
    let params = HelloParams {
        protocol: zamin_protocol::PROTOCOL_VERSION,
        auth: auth.map(str::to_owned),
        client: ClientInfo {
            name: "remote-test".to_owned(),
            version: "0".to_owned(),
        },
    };
    Bytes::from(
        serde_json::to_vec(&Request::new(
            RequestId::Number(id),
            DAEMON_HELLO,
            Some(serde_json::to_value(params).unwrap()),
        ))
        .unwrap(),
    )
}

fn list_frame(id: u64) -> Bytes {
    Bytes::from(
        serde_json::to_vec(&Request::new(
            RequestId::Number(id),
            zamin_protocol::methods::SERVER_LIST,
            None,
        ))
        .unwrap(),
    )
}

/// A minimal daemon: hello gets a real HelloResult; anything else echoes
/// its method name in the result.
async fn stub_daemon(endpoint: Endpoint) {
    let mut server = IpcServer::bind(endpoint).await.unwrap();
    tokio::spawn(async move {
        loop {
            let Ok(conn) = server.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut conn = conn;
                while let Ok(Some(frame)) = conn.recv().await {
                    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&frame) else {
                        continue;
                    };
                    let Some(IncomingMessage::Request(request)) = IncomingMessage::parse(&value)
                    else {
                        continue;
                    };
                    let body = if request.method == DAEMON_HELLO {
                        serde_json::json!({
                            "protocol": 1, "protocolMin": 1, "protocolMax": 1,
                            "daemon": {"name": "stubd", "version": "0"}
                        })
                    } else {
                        serde_json::json!({ "echo": request.method })
                    };
                    let reply = Response::ok(request.id, body);
                    let _ = conn
                        .send(Bytes::from(serde_json::to_vec(&reply).unwrap()))
                        .await;
                }
            });
        }
    });
}

/// One agent bound to an ephemeral port; returns addr + fingerprint.
async fn start_agent(tag: &str, endpoint: Endpoint) -> (SocketAddr, String) {
    let dir = temp_dir(tag);
    let material = TlsMaterial::load_or_generate(&dir.join("tls")).unwrap();
    let config = Arc::new(material.server_config().unwrap());
    let fingerprint = material.fingerprint_hex();
    let server = AgentServer::bind(
        "127.0.0.1:0".parse().unwrap(),
        config,
        endpoint,
        TOKEN.to_owned(),
    )
    .await
    .unwrap();
    let addr = server.local_addr();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    (addr, fingerprint)
}

fn remote_cfg(addr: SocketAddr, token: &str, fingerprint: &str) -> RemoteConnect {
    RemoteConnect {
        addr: addr.to_string(),
        token: token.to_owned(),
        trust: Trust::Fingerprint(fingerprint.to_owned()),
    }
}

#[tokio::test]
async fn authenticated_hello_round_trips_and_relays() {
    let endpoint = Endpoint::unique_for_test("agent-ok");
    stub_daemon(endpoint.clone()).await;
    let (addr, fingerprint) = start_agent("ok", endpoint).await;

    let mut conn = client::connect(&remote_cfg(addr, TOKEN, &fingerprint))
        .await
        .unwrap();
    let hello = client::handshake(
        &mut conn,
        TOKEN,
        ClientInfo {
            name: "remote-test".to_owned(),
            version: "0".to_owned(),
        },
    )
    .await
    .unwrap();
    assert_eq!(hello.daemon.name, "stubd");

    // Post-handshake traffic relays both ways.
    conn.send(list_frame(11)).await.unwrap();
    let reply = reply_of(conn.recv().await.unwrap().unwrap());
    assert_eq!(reply.id, RequestId::Number(11));
    assert_eq!(reply.result.unwrap()["echo"], "server.list");
}

#[tokio::test]
async fn wrong_token_gets_typed_rejection_then_close() {
    let endpoint = Endpoint::unique_for_test("agent-wrong");
    stub_daemon(endpoint.clone()).await;
    let (addr, fingerprint) = start_agent("wrong", endpoint).await;

    let mut conn = client::connect(&remote_cfg(addr, "not-the-token", &fingerprint))
        .await
        .unwrap();
    conn.send(hello_frame(Some("not-the-token"), 1))
        .await
        .unwrap();
    let reply = reply_of(conn.recv().await.unwrap().unwrap());
    assert_eq!(reply.error.unwrap().code, ErrorCode::AuthRejected);
    // The gate closes the connection. rustls does not emit close_notify on
    // drop, so the peer sees either a clean end-of-stream or an EOF error.
    match conn.recv().await {
        Ok(None) => {}
        Err(zamin_ipc::IpcError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {}
        other => panic!("expected connection close, got {other:?}"),
    }
}

#[tokio::test]
async fn missing_auth_gets_auth_required() {
    let endpoint = Endpoint::unique_for_test("agent-noauth");
    stub_daemon(endpoint.clone()).await;
    let (addr, fingerprint) = start_agent("noauth", endpoint).await;

    let mut conn = client::connect(&remote_cfg(addr, "", &fingerprint))
        .await
        .unwrap();
    conn.send(hello_frame(None, 2)).await.unwrap();
    let reply = reply_of(conn.recv().await.unwrap().unwrap());
    assert_eq!(reply.error.unwrap().code, ErrorCode::AuthRequired);
}

#[tokio::test]
async fn unreachable_daemon_gets_typed_error() {
    // No daemon bound on this endpoint.
    let endpoint = Endpoint::unique_for_test("agent-dead");
    let (addr, fingerprint) = start_agent("dead", endpoint).await;

    let mut conn = client::connect(&remote_cfg(addr, TOKEN, &fingerprint))
        .await
        .unwrap();
    conn.send(hello_frame(Some(TOKEN), 3)).await.unwrap();
    let reply = reply_of(conn.recv().await.unwrap().unwrap());
    assert_eq!(reply.error.unwrap().code, ErrorCode::DaemonUnreachable);
}

#[tokio::test]
async fn non_hello_first_frame_is_rejected_before_the_daemon() {
    let endpoint = Endpoint::unique_for_test("agent-order");
    stub_daemon(endpoint.clone()).await;
    let (addr, fingerprint) = start_agent("order", endpoint).await;

    let mut conn = client::connect(&remote_cfg(addr, TOKEN, &fingerprint))
        .await
        .unwrap();
    conn.send(list_frame(4)).await.unwrap();
    let reply = reply_of(conn.recv().await.unwrap().unwrap());
    assert_eq!(
        reply.error.unwrap().code,
        ErrorCode::ProtocolVersionUnsupported
    );
}

#[tokio::test]
async fn plaintext_client_gets_no_response() {
    let endpoint = Endpoint::unique_for_test("agent-plain");
    stub_daemon(endpoint.clone()).await;
    let (addr, _fingerprint) = start_agent("plain", endpoint).await;

    // A plaintext peer cannot complete the TLS handshake. Whatever comes
    // back is at most a TLS alert record — never protocol data.
    let mut tcp = TcpStream::connect(addr).await.unwrap();
    use tokio::io::AsyncWriteExt;
    tcp.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
    tcp.flush().await.unwrap();
    let mut buf = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(5), {
        use tokio::io::AsyncReadExt;
        tcp.read_to_end(&mut buf)
    })
    .await
    .expect("connection ends promptly");
    let _ = read;
    assert!(buf.len() < 64, "too much data for an alert record: {buf:?}");
    assert!(
        serde_json::from_slice::<serde_json::Value>(&buf).is_err(),
        "a plaintext peer must never receive protocol frames, got {buf:?}"
    );
}

#[tokio::test]
async fn skip_verify_trust_is_an_explicit_working_mode() {
    let endpoint = Endpoint::unique_for_test("agent-skip");
    stub_daemon(endpoint.clone()).await;
    let (addr, _fingerprint) = start_agent("skip", endpoint).await;

    let cfg = RemoteConnect {
        addr: addr.to_string(),
        token: TOKEN.to_owned(),
        trust: Trust::InsecureSkipVerify,
    };
    let mut conn = client::connect(&cfg).await.unwrap();
    let hello = client::handshake(
        &mut conn,
        TOKEN,
        ClientInfo {
            name: "remote-test".to_owned(),
            version: "0".to_owned(),
        },
    )
    .await
    .unwrap();
    assert_eq!(hello.daemon.name, "stubd");
}

#[tokio::test]
async fn wrong_fingerprint_fails_the_tls_handshake() {
    let endpoint = Endpoint::unique_for_test("agent-pin");
    stub_daemon(endpoint.clone()).await;
    let (addr, _fingerprint) = start_agent("pin", endpoint).await;

    let other = zamin_agent::tls::TlsMaterial::load_or_generate(&temp_dir("pin-other"))
        .unwrap()
        .fingerprint_hex();
    let err = client::connect(&remote_cfg(addr, TOKEN, &other)).await;
    assert!(err.is_err(), "a mismatched fingerprint must not connect");
}

fn reply_of(frame: Bytes) -> Response {
    serde_json::from_slice(&frame).unwrap()
}
