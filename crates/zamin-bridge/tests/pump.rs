//! Bridge integration: a real IPC connection, the pump coalescing
//! daemon-side frames into batches, and the down signal on wire death.

use std::time::Duration;

use bytes::Bytes;
use tokio::sync::mpsc;
use zamin_bridge::{send_frame, spawn_read};
use zamin_ipc::{connect, IpcServer};

async fn recv_batch(receiver: &mut mpsc::Receiver<Vec<String>>, timeout: Duration) -> Vec<String> {
    tokio::time::timeout(timeout, receiver.recv())
        .await
        .expect("a batch arrives")
        .expect("channel stays open")
}

#[tokio::test(flavor = "multi_thread")]
async fn daemon_frames_arrive_coalesced_and_down_fires_on_close() {
    let endpoint = zamin_ipc::Endpoint::unique_for_test("bridge-pump");
    let mut server = IpcServer::bind(endpoint.clone())
        .await
        .expect("bind test endpoint");

    let connection = connect(endpoint).await.expect("client connects");
    let (_write_half, read_half) = connection.split();

    let (batch_tx, mut batch_rx) = mpsc::channel(64);
    let (down_tx, mut down_rx) = mpsc::channel::<()>(1);
    let _pump = spawn_read(read_half, batch_tx, move || {
        down_tx.try_send(()).expect("down signal delivered");
    });

    // Give the daemon side a moment to accept the connection.
    let mut daemon_side = server.accept().await.expect("accept");

    // Four frames in quick succession: they must coalesce, not stream.
    // (Connection::send frames the payload; no manual framing here.)
    for i in 0..4 {
        let frame = format!(r#"{{"i":{i}}}"#);
        daemon_side.send(Bytes::from(frame)).await.expect("send");
    }

    // One coalesced batch (or a small number if the window straddles) —
    // never one message per frame.
    let first = recv_batch(&mut batch_rx, Duration::from_secs(2)).await;
    assert!(first.len() >= 2, "frames coalesce, got {:?}", first);
    assert!(first.iter().all(|frame| frame.starts_with("{\"i\":")));

    // Dropping the daemon side fires the down signal exactly once.
    drop(daemon_side);
    drop(server);
    tokio::time::timeout(Duration::from_secs(2), down_rx.recv())
        .await
        .expect("down fires")
        .expect("down signal");
}

#[tokio::test(flavor = "multi_thread")]
async fn send_frame_reaches_the_daemon() {
    let endpoint = zamin_ipc::Endpoint::unique_for_test("bridge-send");
    let mut server = IpcServer::bind(endpoint.clone())
        .await
        .expect("bind test endpoint");

    let connection = connect(endpoint).await.expect("client connects");
    let (mut write_half, _read_half) = connection.split();
    send_frame(&mut write_half, r#"{"jsonrpc":"2.0","id":1}"#)
        .await
        .expect("send");

    let mut daemon_side = server.accept().await.expect("accept");
    let frame = tokio::time::timeout(Duration::from_secs(2), daemon_side.recv())
        .await
        .expect("frame arrives")
        .expect("open")
        .expect("frame");
    assert_eq!(frame, Bytes::from_static(br#"{"jsonrpc":"2.0","id":1}"#));
}
