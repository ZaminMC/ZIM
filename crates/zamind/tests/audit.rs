//! The audit log end-to-end: handshakes and mutating commands land in
//! `<data>/audit.log` as JSONL with the protocol client as the actor and
//! the daemon's outcome; reads stay out; a rejected handshake is recorded.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use serde_json::{json, Value};
use zamin_protocol::methods;
use zamin_protocol::server::ServerState;

mod common;

use common::{
    connect_daemon, make_server_root, register_server, scoped_dir, spawn_daemon, start_server,
    subscribe_events, wait_for_state, write_server_config,
};

fn audit_lines(data_dir: &std::path::Path) -> Vec<Value> {
    let content = std::fs::read_to_string(data_dir.join("audit.log"))
        .expect("audit log exists after a session");
    content
        .lines()
        .map(|line| serde_json::from_str(line).expect("each audit line is JSON"))
        .collect()
}

#[tokio::test]
async fn handshakes_and_mutations_are_audited_reads_are_not() {
    let data_dir = scoped_dir("audit");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("audit");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    subscribe_events(&mut client, Some("test")).await;

    let root = make_server_root("audit-root");
    register_server(&mut client, "test", &root).await;
    write_server_config(&data_dir, "test", "port = 0\n");
    start_server(&mut client, "test")
        .await
        .expect("start accepted");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("running");
    client
        .request(
            methods::SERVER_STOP,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("stop accepted");
    wait_for_state(&mut client, ServerState::Stopped, Duration::from_secs(15))
        .await
        .expect("stopped");

    // A read: answered, never audited.
    let _list: Value = client
        .request(methods::SERVER_LIST, json!({}))
        .await
        .expect("list answers");

    // A rejected handshake: bad protocol version, its own connection.
    let mut raw = zamin_ipc::connect(endpoint.clone())
        .await
        .expect("raw connect");
    let bad_hello = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": methods::DAEMON_HELLO,
        "params": {
            "protocol": 9_999,
            "client": { "name": "audit-probe", "version": "0" },
        },
    });
    raw.send(bytes::Bytes::from(serde_json::to_vec(&bad_hello).unwrap()))
        .await
        .expect("bad hello sent");
    let reply = raw.recv().await.expect("reply").expect("reply frame");
    let reply: Value = serde_json::from_slice(&reply).unwrap();
    assert_eq!(
        reply["error"]["code"], "PROTOCOL_VERSION_UNSUPPORTED",
        "the daemon rejects the version mistmatch"
    );
    drop(raw);

    let lines = audit_lines(&data_dir);
    let hellos: Vec<&Value> = lines
        .iter()
        .filter(|l| l["method"] == "daemon.hello")
        .collect();
    assert!(
        hellos
            .iter()
            .any(|l| l["outcome"] == "ok" && l["client"]["name"] == "zamind-test"),
        "accepted hellos record the protocol client: {hellos:?}"
    );
    assert!(
        hellos.iter().any(|l| l["outcome"] == "rejected"),
        "the rejected handshake is recorded"
    );

    for audited in ["server.register", "server.start", "server.stop"] {
        assert!(
            lines.iter().any(|l| l["method"] == audited
                && l["serverId"] == "test"
                && l["outcome"] == "ok"
                && l["client"]["name"] == "zamind-test"),
            "{audited} is audited with server, outcome and client"
        );
    }

    assert!(
        !lines.iter().any(|l| l["method"] == "server.list"),
        "reads are not audited"
    );
}
