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

    // A mutating plugins command is audited too — even when it fails
    // (the file does not exist; the daemon's outcome is still recorded).
    let _deleted = client
        .request(
            methods::PLUGINS_DELETE,
            json!({"serverId": "test", "fileName": "not-there.jar"}),
        )
        .await;

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
        lines.iter().any(|l| l["method"] == "plugins.delete"
            && l["serverId"] == "test"
            && l["client"]["name"] == "zamind-test"),
        "the mutating plugins command is audited: {lines:?}"
    );

    assert!(
        !lines.iter().any(|l| l["method"] == "server.list"),
        "reads are not audited"
    );
}

#[tokio::test]
async fn audit_list_reads_back_what_the_session_wrote() {
    // The read side (ADR-0011): a session that only ever saw write-only
    // evidence now answers audit.list — newest-first, refusals carrying
    // their codes, the actor named honestly, the listing itself never
    // audited, and a line the file cannot parse counted, not dropped.
    let data_dir = scoped_dir("audit-list");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("audit-list");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("audit-list-root");
    register_server(&mut client, "test", &root).await;
    // A refused mutation: its outcome code is evidence too.
    let _refused = client
        .request(
            methods::SERVER_START,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "ghost"}),
        )
        .await;

    // A garbage line lands the way a broken rotation could leave one.
    {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(data_dir.join("audit.log"))
            .expect("audit log exists after mutations");
        writeln!(file, "{{\"tsMs\": torn").expect("garbage append");
    }

    let result: Value = client
        .request(methods::AUDIT_LIST, json!({ "limit": 50 }))
        .await
        .expect("audit.list answers");
    let entries = result["entries"].as_array().expect("entries array");
    assert_eq!(result["malformed"], 1, "the torn line is counted");
    assert!(
        !entries.iter().any(|e| e["method"] == "audit.list"),
        "the listing itself is not audited"
    );

    // Newest first: the last mutation before the read answers first.
    assert_eq!(entries[0]["method"], "server.start");
    assert_eq!(entries[0]["outcome"], "SERVER_NOT_FOUND");
    assert_eq!(entries[0]["serverId"], "ghost");
    assert_eq!(entries[0]["client"]["name"], "zamind-test");

    let register_at = entries
        .iter()
        .position(|e| e["method"] == "server.register")
        .expect("the register is in the page");
    assert!(
        register_at > 0,
        "register happened before the refused start, so it sits deeper"
    );
    assert_eq!(entries[register_at]["outcome"], "ok");

    // Paging: the second page holds the session's older tail, and the
    // newest entry never repeats.
    let second: Value = client
        .request(methods::AUDIT_LIST, json!({ "limit": 1, "offset": 1 }))
        .await
        .expect("paged audit.list answers");
    let second_entries = second["entries"].as_array().unwrap();
    assert_eq!(second_entries.len(), 1);
    assert_ne!(second_entries[0]["method"], "server.start");
    assert_eq!(
        second_entries[0]["method"], "server.register",
        "offset 1 lands one below the newest"
    );
}
