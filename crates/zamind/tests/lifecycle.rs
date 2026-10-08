//! Lifecycle matrix integration tests (TESTING.md, ADR-0005): every
//! scenario the fake-mc-server was built for, driven over the real
//! protocol against the real daemon binary. Complements the happy-path
//! e2e in daemon.rs with the error paths the first release omitted.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use serde_json::json;

use zamin_protocol::methods;
use zamin_protocol::server::ServerState;

use common::{
    connect_daemon, make_server_root, register_server, scoped_dir, spawn_daemon, start_server,
    subscribe_events, wait_for_state, wait_list_state, write_server_config,
};

#[tokio::test(flavor = "multi_thread")]
async fn startup_crash_is_reported_with_classification() {
    let data_dir = scoped_dir("data-startup-crash");
    let root = make_server_root("root-startup-crash");
    write_server_config(
        &data_dir,
        "test",
        "extraJvmArgs = [\"--fail-boot\", \"--exit-code\", \"3\"]\n",
    );

    let endpoint = zamin_ipc::Endpoint::unique_for_test("startup-crash");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;
    subscribe_events(&mut client, Some("test")).await;

    start_server(&mut client, "test")
        .await
        .expect("start accepted");

    let crashed = wait_for_state(&mut client, ServerState::Crashed, Duration::from_secs(15))
        .await
        .expect("server reaches crashed");
    assert_eq!(crashed["crash"]["phase"], "startup");
    assert_eq!(crashed["crash"]["exitCode"], 3);
    // The crash card's evidence excerpt: the last ingested log lines before
    // the process died (the fake server's FATAL line).
    let evidence = crashed["crash"]["evidence"]
        .as_str()
        .expect("crash carries an evidence excerpt");
    assert!(
        evidence.contains("Failed to start the minecraft server"),
        "evidence should contain the FATAL line, got: {evidence}"
    );

    let entry = wait_list_state(&mut client, "test", "crashed", Duration::from_secs(5)).await;
    assert_eq!(entry["state"], "crashed");
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_crash_reports_crash_card() {
    let data_dir = scoped_dir("data-runtime-crash");
    let root = make_server_root("root-runtime-crash");
    write_server_config(
        &data_dir,
        "test",
        "extraJvmArgs = [\"--crash-mid-run\", \"--exit-code\", \"7\"]\n",
    );

    let endpoint = zamin_ipc::Endpoint::unique_for_test("runtime-crash");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;
    subscribe_events(&mut client, Some("test")).await;

    start_server(&mut client, "test")
        .await
        .expect("start accepted");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("server reaches running");

    let crashed = wait_for_state(&mut client, ServerState::Crashed, Duration::from_secs(15))
        .await
        .expect("server reaches crashed");
    assert_eq!(crashed["crash"]["phase"], "runtime");
    assert_eq!(crashed["crash"]["exitCode"], 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn kill_is_a_deliberate_stop_not_a_crash() {
    let data_dir = scoped_dir("data-kill");
    let root = make_server_root("root-kill");
    write_server_config(&data_dir, "test", "");

    let endpoint = zamin_ipc::Endpoint::unique_for_test("kill");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;
    subscribe_events(&mut client, Some("test")).await;

    start_server(&mut client, "test")
        .await
        .expect("start accepted");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("server reaches running");

    client
        .request(
            methods::SERVER_KILL,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("kill accepted");

    let stopping = wait_for_state(&mut client, ServerState::Stopping, Duration::from_secs(10))
        .await
        .expect("server reaches stopping");
    assert_eq!(stopping["reason"], "kill-requested");

    let stopped = wait_for_state(&mut client, ServerState::Stopped, Duration::from_secs(15))
        .await
        .expect("server reaches stopped");
    assert_eq!(stopped["reason"], "process-exited");
    assert!(
        stopped["crash"].is_null(),
        "a user kill must never surface a crash card: {stopped}"
    );
    wait_list_state(&mut client, "test", "stopped", Duration::from_secs(5)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_ladder_escalates_past_stubborn_stdin() {
    let data_dir = scoped_dir("data-ladder");
    let root = make_server_root("root-ladder");
    // The fake server ignores the stdin stop; the ladder must escalate to
    // the OS-graceful signal after stop_timeout (1s here) and reach the
    // force step if even that fails. A plain process dies on the signal.
    write_server_config(
        &data_dir,
        "test",
        "stopTimeoutSecs = 1\nextraJvmArgs = [\"--ignore-stop\"]\n",
    );

    let endpoint = zamin_ipc::Endpoint::unique_for_test("ladder");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;
    subscribe_events(&mut client, Some("test")).await;

    start_server(&mut client, "test")
        .await
        .expect("start accepted");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("server reaches running");

    client
        .request(
            methods::SERVER_STOP,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("stop accepted");

    let stopped = wait_for_state(&mut client, ServerState::Stopped, Duration::from_secs(20))
        .await
        .expect("stubborn server still reaches stopped via the ladder");
    assert!(stopped["crash"].is_null(), "ladder stop is not a crash");
}

#[tokio::test(flavor = "multi_thread")]
async fn eula_preflight_failure_recovers_after_accept() {
    let data_dir = scoped_dir("data-eula");
    let root = scoped_dir("root-eula");
    // No eula.txt: the classic first-run trap must be a typed preflight
    // failure, and accepting it afterwards must let the server start.
    std::fs::write(root.join("server.jar"), b"fake jar bytes").unwrap();
    write_server_config(&data_dir, "test", "");

    let endpoint = zamin_ipc::Endpoint::unique_for_test("eula");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;
    subscribe_events(&mut client, Some("test")).await;

    let error = start_server(&mut client, "test")
        .await
        .expect_err("start without eula must fail preflight");
    assert_eq!(error["code"], "NEEDS_EULA", "{error}");
    wait_list_state(
        &mut client,
        "test",
        "failed-preflight",
        Duration::from_secs(5),
    )
    .await;

    std::fs::write(root.join("eula.txt"), "eula=true\n").unwrap();
    start_server(&mut client, "test")
        .await
        .expect("start after accepting eula");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("server reaches running after eula acceptance");
}

#[tokio::test(flavor = "multi_thread")]
async fn adopted_server_survives_daemon_death_and_can_be_stopped() {
    // The M1 regression: an adopted server was unsupervisable — stop and
    // kill both answered ServerNotRunning and removal orphaned the JVM.
    let data_dir = scoped_dir("data-adopt");
    let root = make_server_root("root-adopt");
    write_server_config(&data_dir, "test", "");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("adopt");

    // Daemon A: start the server, then die hard (SIGKILL; no cleanups).
    {
        let daemon_a = spawn_daemon(&data_dir, &endpoint);
        let mut client = connect_daemon(&endpoint).await;
        register_server(&mut client, "test", &root).await;
        start_server(&mut client, "test")
            .await
            .expect("start accepted");
        wait_list_state(&mut client, "test", "running", Duration::from_secs(15)).await;
        drop(client);
        drop(daemon_a); // tree kill takes only the daemon (own process group)
    }

    // Daemon B: same data dir, same endpoint (stale socket must be
    // reclaimed). The recorded runtime identity verifies, so the server
    // comes back as running — adopted, not respawned.
    let _daemon_b = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    let adopted = wait_list_state(&mut client, "test", "running", Duration::from_secs(15)).await;
    assert_eq!(adopted["state"], "running");

    // The whole point: an adopted server must be stoppable.
    subscribe_events(&mut client, Some("test")).await;
    client
        .request(
            methods::SERVER_STOP,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("stop of adopted server accepted");
    wait_for_state(&mut client, ServerState::Stopped, Duration::from_secs(20))
        .await
        .expect("adopted server reaches stopped");
    wait_list_state(&mut client, "test", "stopped", Duration::from_secs(5)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_start_is_idempotent() {
    let data_dir = scoped_dir("data-dupstart");
    let root = make_server_root("root-dupstart");
    write_server_config(&data_dir, "test", "");

    let endpoint = zamin_ipc::Endpoint::unique_for_test("dupstart");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;
    subscribe_events(&mut client, Some("test")).await;

    let first = start_server(&mut client, "test")
        .await
        .expect("first start");
    assert_eq!(first["state"], "starting");
    let second = start_server(&mut client, "test")
        .await
        .expect("second start");
    let second_state = second["state"].clone();
    assert!(
        second_state == "starting" || second_state == "running",
        "duplicate start must be idempotent, got {second_state}"
    );

    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("server reaches running exactly once supervised");
    wait_list_state(&mut client, "test", "running", Duration::from_secs(5)).await;

    client
        .request(
            methods::SERVER_STOP,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("stop accepted");
    wait_for_state(&mut client, ServerState::Stopped, Duration::from_secs(15))
        .await
        .expect("stops cleanly after duplicate start");
}

#[tokio::test(flavor = "multi_thread")]
async fn java_incompatible_preflight_is_typed() {
    // The fake server always reports java.version = 21.0.3; requiring a
    // newer major must produce a typed JAVA_INCOMPATIBLE preflight
    // failure, not a crash or a boot attempt.
    let data_dir = scoped_dir("data-java");
    let root = make_server_root("root-java");
    write_server_config(&data_dir, "test", "javaMajorRequired = 25\n");

    let endpoint = zamin_ipc::Endpoint::unique_for_test("java");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;
    subscribe_events(&mut client, Some("test")).await;

    let error = start_server(&mut client, "test")
        .await
        .expect_err("start with incompatible java must fail preflight");
    assert_eq!(error["code"], "JAVA_INCOMPATIBLE", "{error}");
    assert_eq!(
        error["context"]["found"], 21,
        "found major comes from the runtime inspection"
    );
    assert_eq!(error["context"]["required"], 25);
    wait_list_state(
        &mut client,
        "test",
        "failed-preflight",
        Duration::from_secs(5),
    )
    .await;

    // Recovery: relaxing the requirement lets the same server start.
    write_server_config(&data_dir, "test", "javaMajorRequired = 17\n");
    start_server(&mut client, "test")
        .await
        .expect("start after relaxing requirement");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("server reaches running with satisfied requirement");
}

/// Wire robustness (JSON-RPC 2.0 §4): explicit `id: null` requests stay
/// legal, unreadable ids answer PROTOCOL_INVALID_REQUEST with a null id,
/// unknown methods answer PROTOCOL_METHOD_NOT_FOUND, and notifications
/// never draw a reply.
#[tokio::test(flavor = "multi_thread")]
async fn wire_robustness_null_ids_invalid_requests_and_unknown_methods() {
    let data_dir = scoped_dir("data-wire-robustness");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("wire-robustness");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    use bytes::Bytes;
    use serde_json::Value;

    async fn send_raw(client: &mut common::Client, value: serde_json::Value) {
        let payload = serde_json::to_vec(&value).unwrap();
        client
            .connection
            .send(Bytes::from(payload))
            .await
            .expect("send raw frame");
    }

    // 1. `id: null` is a legal request; the response echoes the null id.
    send_raw(
        &mut client,
        json!({"jsonrpc": "2.0", "id": null, "method": methods::DAEMON_STATUS}),
    )
    .await;
    let frame = next_response_frame(&mut client).await;
    let value: Value = serde_json::from_slice(&frame).unwrap();
    assert!(
        value["id"].is_null(),
        "response must echo the null id: {value}"
    );
    assert!(
        value["result"].is_object(),
        "status result present: {value}"
    );

    // 2. An unreadable id (an object) is a PROTOCOL_INVALID_REQUEST replied
    //    with a null id — never silence, never a guessed id.
    send_raw(
        &mut client,
        json!({"jsonrpc": "2.0", "id": {"bad": true}, "method": methods::DAEMON_STATUS}),
    )
    .await;
    let frame = next_response_frame(&mut client).await;
    let value: Value = serde_json::from_slice(&frame).unwrap();
    assert!(
        value["id"].is_null(),
        "error reply carries id: null: {value}"
    );
    assert_eq!(value["error"]["code"], "PROTOCOL_INVALID_REQUEST");

    // 3. Unknown method → PROTOCOL_METHOD_NOT_FOUND (not INTERNAL_ERROR).
    //    jobs.get is a real method now: it answers JOB_NOT_FOUND for a
    //    nonexistent job instead of the unimplemented rejection.
    let error = client
        .request(
            methods::JOBS_GET,
            json!({"jobId": "00000000-0000-0000-0000-000000000000"}),
        )
        .await
        .expect_err("no such job exists");
    assert_eq!(error["code"], "JOB_NOT_FOUND");

    let error = client
        .request("definitely.not.a.method", json!({}))
        .await
        .expect_err("unknown method");
    assert_eq!(error["code"], "PROTOCOL_METHOD_NOT_FOUND");

    // 4. A notification (method, no id) gets NO reply: the next response on
    //    the wire must belong to the request that follows it.
    send_raw(
        &mut client,
        json!({"jsonrpc": "2.0", "method": methods::SERVER_LIST, "params": {}}),
    )
    .await;
    client.next_id = 40;
    let status = client
        .request(methods::DAEMON_STATUS, json!({}))
        .await
        .expect("request after a notification gets exactly its own response");
    assert!(status["servers"].is_u64());
}

/// Receive frames, skipping stream notifications, until a JSON-RPC response
/// arrives. This connection holds no subscriptions, so this is only ever
/// the reply to the request just sent.
async fn next_response_frame(client: &mut common::Client) -> bytes::Bytes {
    use serde_json::Value;
    use zamin_protocol::envelope::IncomingMessage;
    loop {
        let frame = client
            .connection
            .recv()
            .await
            .expect("recv")
            .expect("connection open");
        let value: Value = serde_json::from_slice(&frame).unwrap();
        if matches!(
            IncomingMessage::parse(&value),
            Some(IncomingMessage::Response(_))
        ) {
            return frame;
        }
    }
}

/// `logs.range`: the file-backed historical tail (protocol spec §5).
/// Streams serve what the daemon has ingested; the log file holds the
/// history — this is the catch-up path (ADR-0006).
#[tokio::test(flavor = "multi_thread")]
async fn logs_range_tails_the_file_backed_history() {
    let data_dir = scoped_dir("data-log-range");
    let root = make_server_root("root-log-range");
    let log_dir = root.join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let mut contents = String::new();
    for i in 0..50 {
        contents.push_str(&format!(
            "[05:24:{:02}] [Server thread/INFO]: historical line {i}\n",
            i % 60
        ));
    }
    std::fs::write(log_dir.join("latest.log"), &contents).unwrap();

    let endpoint = zamin_ipc::Endpoint::unique_for_test("log-range");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;

    // Tail of 10 out of 50: the LAST ten, with older lines still available.
    let tail = client
        .request(
            methods::LOGS_RANGE,
            json!({"serverId": "test", "maxLines": 10}),
        )
        .await
        .expect("log range tail");
    assert_eq!(tail["file"], "logs/latest.log");
    let lines = tail["lines"].as_array().expect("lines array");
    assert_eq!(lines.len(), 10);
    assert_eq!(lines[0]["line"], "historical line 40");
    assert_eq!(lines[9]["line"], "historical line 49");
    assert_eq!(tail["olderAvailable"], true);

    // Default (no maxLines) serves everything in the file: no older lines.
    let all = client
        .request(methods::LOGS_RANGE, json!({"serverId": "test"}))
        .await
        .expect("log range default");
    assert_eq!(all["lines"].as_array().expect("lines array").len(), 50);
    assert_eq!(all["olderAvailable"], false);

    // Parsed fields ride along (level/thread); tsMs stays 0 for file-backed
    // lines — the ingestion time never existed (protocol spec §9).
    assert_eq!(all["lines"][0]["level"], "info");
    assert_eq!(all["lines"][0]["thread"], "Server thread");
    assert_eq!(all["lines"][0]["tsMs"], 0);

    // A missing log file is NOT a broken console (P0): the server simply
    // has no history yet. The historical read answers honestly empty with
    // historyAvailable: false — the live stream is the console, and this
    // is a state, not an error the client would spam.
    let empty_root = make_server_root("root-log-range-empty");
    register_server(&mut client, "fresh", &empty_root).await;
    let no_history = client
        .request(methods::LOGS_RANGE, json!({"serverId": "fresh"}))
        .await
        .expect("missing history is an honest empty, not an error");
    assert_eq!(no_history["lines"].as_array().expect("lines").len(), 0);
    assert_eq!(no_history["historyAvailable"], false);
    assert_eq!(no_history["olderAvailable"], false);
    // And the file-backed read of a server WITH a log says so:
    assert_eq!(all["historyAvailable"], true);

    // Unregistered server → SERVER_NOT_FOUND.
    let error = client
        .request(methods::LOGS_RANGE, json!({"serverId": "ghost"}))
        .await
        .expect_err("ghost server");
    assert_eq!(error["code"], "SERVER_NOT_FOUND");
}

#[tokio::test(flavor = "multi_thread")]
async fn register_and_remove_are_broadcast_on_the_events_stream() {
    let data_dir = scoped_dir("data-registry-events");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("registry-events");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    // Subscribe BEFORE registering: the registration must arrive as a live
    // event, not only in later snapshots (ADR-0006 reconcile channel).
    subscribe_events(&mut client, None).await;

    let root = make_server_root("root-registry-events");
    register_server(&mut client, "demo", &root).await;
    let registered = wait_for_state(&mut client, ServerState::NotRunning, Duration::from_secs(5))
        .await
        .expect("registered event");
    assert_eq!(registered["serverId"], "demo");
    assert_eq!(registered["from"], "unknown");
    assert_eq!(registered["reason"], "registered");

    client
        .request(
            methods::SERVER_REMOVE,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "demo"}),
        )
        .await
        .expect("remove accepted");
    let removed = wait_for_state(&mut client, ServerState::Unknown, Duration::from_secs(5))
        .await
        .expect("removed event");
    assert_eq!(removed["serverId"], "demo");
    assert_eq!(removed["reason"], "removed");
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_range_pages_backward_by_byte_offset() {
    let data_dir = scoped_dir("data-log-page");
    let root = make_server_root("root-log-page");
    let log_dir = root.join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let mut contents = String::new();
    for i in 0..120 {
        contents.push_str(&format!(
            "[05:30:{:02}] [Server thread/INFO]: page line {i}\n",
            i % 60
        ));
    }
    std::fs::write(log_dir.join("latest.log"), &contents).unwrap();

    let endpoint = zamin_ipc::Endpoint::unique_for_test("log-page");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;

    // Page 1 (tail): the last 40 of 120 lines, with a cursor pointing at
    // the start of the first served line.
    let page1 = client
        .request(
            methods::LOGS_RANGE,
            json!({"serverId": "test", "maxLines": 40}),
        )
        .await
        .expect("tail page");
    let lines1 = page1["lines"].as_array().expect("lines array");
    assert_eq!(lines1.len(), 40);
    assert_eq!(lines1[0]["line"], "page line 80");
    assert_eq!(lines1[39]["line"], "page line 119");
    assert_eq!(page1["olderAvailable"], true);
    let cursor1 = page1["startOffset"].as_u64().expect("startOffset");
    assert!(cursor1 > 0);

    // Page 2: lines ending at or before page 1's first line — exactly the
    // preceding 40, contiguous with page 1.
    let page2 = client
        .request(
            methods::LOGS_RANGE,
            json!({"serverId": "test", "maxLines": 40, "beforeOffset": cursor1}),
        )
        .await
        .expect("second page");
    let lines2 = page2["lines"].as_array().expect("lines array");
    assert_eq!(lines2.len(), 40);
    assert_eq!(lines2[0]["line"], "page line 40");
    assert_eq!(lines2[39]["line"], "page line 79");
    assert_eq!(page2["olderAvailable"], true);
    let cursor2 = page2["startOffset"].as_u64().expect("startOffset");
    assert!(cursor2 > 0 && cursor2 < cursor1);

    // Page 3: back to the file start; nothing older remains.
    let page3 = client
        .request(
            methods::LOGS_RANGE,
            json!({"serverId": "test", "maxLines": 40, "beforeOffset": cursor2}),
        )
        .await
        .expect("third page");
    let lines3 = page3["lines"].as_array().expect("lines array");
    assert_eq!(lines3.len(), 40);
    assert_eq!(lines3[0]["line"], "page line 0");
    assert_eq!(lines3[39]["line"], "page line 39");
    assert_eq!(page3["olderAvailable"], false);
    assert_eq!(page3["startOffset"], 0);

    // Cursor 0 is a valid exhausted cursor: an honest empty page.
    let exhausted = client
        .request(
            methods::LOGS_RANGE,
            json!({"serverId": "test", "maxLines": 40, "beforeOffset": 0}),
        )
        .await
        .expect("exhausted cursor");
    assert_eq!(exhausted["lines"].as_array().expect("lines array").len(), 0);
    assert_eq!(exhausted["olderAvailable"], false);
    assert_eq!(exhausted["startOffset"], 0);

    // A cursor past the current end of the file means rotation or
    // truncation happened: typed error, never garbled pages.
    let stale = client
        .request(
            methods::LOGS_RANGE,
            json!({"serverId": "test", "maxLines": 40, "beforeOffset": contents.len() as u64 + 999}),
        )
        .await
        .expect_err("stale cursor");
    assert_eq!(stale["code"], "LOG_CURSOR_INVALID");
}

#[tokio::test(flavor = "multi_thread")]
async fn files_surface_manages_a_server_root_over_the_wire() {
    let data_dir = scoped_dir("data-files");
    let root = make_server_root("root-files");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("files");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;

    // mkdir -p through the rooted filesystem.
    let ok = client
        .request(
            methods::FILES_MKDIR,
            json!({"serverId": "test", "path": "plugins/EssentialsX"}),
        )
        .await
        .expect("mkdir");
    assert!(ok.as_object().expect("empty result object").is_empty());
    assert!(root.join("plugins/EssentialsX").is_dir());

    // Staged upload: two chunks, then commit with one atomic rename.
    let staged = client
        .request(
            methods::FILES_WRITE,
            // base64("hello ")
            json!({"serverId": "test", "content": "aGVsbG8g"}),
        )
        .await
        .expect("write chunk 1");
    let staging_id = staged["stagingId"].as_str().expect("staging id").to_owned();
    assert!(staging_id.starts_with("stage-"));

    let staged = client
        .request(
            methods::FILES_WRITE,
            // base64("world!")
            json!({"serverId": "test", "stagingId": staging_id, "content": "d29ybGQh"}),
        )
        .await
        .expect("write chunk 2");
    assert_eq!(staged["bytesStaged"], 12);

    let committed = client
        .request(
            methods::FILES_COMMIT,
            json!({"serverId": "test", "stagingId": staging_id, "target": "plugins/EssentialsX/config.yml"}),
        )
        .await
        .expect("commit");
    assert_eq!(committed["path"], "plugins/EssentialsX/config.yml");
    assert_eq!(committed["sizeBytes"], 12);
    assert_eq!(
        std::fs::read(root.join("plugins/EssentialsX/config.yml")).unwrap(),
        b"hello world!"
    );
    // The staging handle is consumed: committing it again is a typed miss.
    let error = client
        .request(
            methods::FILES_COMMIT,
            json!({"serverId": "test", "stagingId": staging_id, "target": "again.txt"}),
        )
        .await
        .expect_err("staging gone");
    assert_eq!(error["code"], "FS_NOT_FOUND");

    // Paged listing: directories first, totals across pages.
    let listed = client
        .request(
            methods::FILES_LIST,
            json!({"serverId": "test", "path": "plugins", "offset": 0, "limit": 1}),
        )
        .await
        .expect("list page 1");
    assert_eq!(listed["total"], 1); // just the EssentialsX directory so far
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    assert_eq!(listed["entries"][0]["kind"], "directory");

    // Chunked read with eof and total size.
    let chunk = client
        .request(
            methods::FILES_READ,
            json!({"serverId": "test", "path": "plugins/EssentialsX/config.yml", "offset": 6, "maxBytes": 100}),
        )
        .await
        .expect("read chunk");
    assert_eq!(chunk["totalBytes"], 12);
    let decoded = base64_decode(chunk["data"].as_str().expect("data"));
    assert_eq!(decoded, b"world!");
    assert!(chunk["eof"].as_bool().unwrap());

    // Rename and delete, both root-contained.
    client
        .request(
            methods::FILES_RENAME,
            json!({"serverId": "test", "from": "plugins/EssentialsX/config.yml", "to": "plugins/config.yml"}),
        )
        .await
        .expect("rename");
    assert!(root.join("plugins/config.yml").is_file());

    client
        .request(
            methods::FILES_DELETE,
            json!({"serverId": "test", "path": "plugins/config.yml"}),
        )
        .await
        .expect("delete file");
    assert!(!root.join("plugins/config.yml").exists());

    // `..` never reaches the disk.
    let error = client
        .request(
            methods::FILES_READ,
            json!({"serverId": "test", "path": "../../../etc/passwd", "offset": 0, "maxBytes": 10}),
        )
        .await
        .expect_err("escape");
    assert_eq!(error["code"], "FS_PATH_ESCAPES_ROOT");

    // A symlink pointing outside is listed but denied.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc", root.join("plugins/escape-link")).unwrap();
        let listed = client
            .request(
                methods::FILES_LIST,
                json!({"serverId": "test", "path": "plugins"}),
            )
            .await
            .expect("list with symlink");
        let entries = listed["entries"].as_array().unwrap();
        let link = entries
            .iter()
            .find(|e| e["name"] == "escape-link")
            .expect("symlink listed");
        assert_eq!(link["symlinkOutside"], true);
    }

    // Unregistered server → SERVER_NOT_FOUND, before any filesystem touch.
    let error = client
        .request(
            methods::FILES_LIST,
            json!({"serverId": "ghost", "path": "."}),
        )
        .await
        .expect_err("ghost");
    assert_eq!(error["code"], "SERVER_NOT_FOUND");
}

fn base64_decode(value: &str) -> Vec<u8> {
    // The test suite has no base64 dependency; decode the standard
    // alphabet by hand for the one place that needs it.
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let trim = value.trim_end_matches('=');
    let mut out = Vec::new();
    for chunk in trim.as_bytes().chunks(4) {
        let mut acc = 0u32;
        for (i, b) in chunk.iter().enumerate() {
            let v = ALPHABET
                .iter()
                .position(|a| a == b)
                .expect("valid base64 char") as u32;
            acc |= v << (18 - 6 * i);
        }
        let bytes = chunk.len();
        if bytes >= 2 {
            out.push((acc >> 16) as u8);
        }
        if bytes >= 3 {
            out.push((acc >> 8) as u8);
        }
        if bytes >= 4 {
            out.push(acc as u8);
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn players_list_pings_the_running_server() {
    let data_dir = scoped_dir("data-players");
    let root = make_server_root("root-players");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("players");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "test", &root).await;

    // No port configured: the honest typed refusal, not a fake empty room.
    let error = client
        .request(methods::PLAYERS_LIST, json!({"serverId": "test"}))
        .await
        .expect_err("no port");
    assert_eq!(error["code"], "PROTOCOL_INVALID_REQUEST");

    // Configure a port, start the fake server, and ping it for real.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    write_server_config(&data_dir, "test", &format!("port = {port}\n"));
    // The fake server binds what Paper would: server.properties in its root.
    std::fs::write(
        root.join("server.properties"),
        format!("server-port={port}\n"),
    )
    .unwrap();
    start_server(&mut client, "test")
        .await
        .expect("start accepted");
    wait_list_state(&mut client, "test", "running", Duration::from_secs(30)).await;

    // A running fake server answers the ping — the fake binds its port
    // before its first stdout line, so "running" already implies listening.
    // Under extreme machine load a single attempt can still burn the
    // daemon's whole PING_TIMEOUT (5 s) and surface as an honest empty
    // room; the semantics under test are "a live server answers", so a
    // few attempts are fair. A dead port never starts answering, and the
    // stopped-server assertion below stays strict.
    let mut result = None;
    for attempt in 0..3 {
        let attempt_result = client
            .request(methods::PLAYERS_LIST, json!({"serverId": "test"}))
            .await
            .expect("players ping");
        if attempt_result["online"].as_u64() == Some(1) {
            result = Some(attempt_result);
            break;
        }
        assert!(
            attempt + 1 < 3,
            "running server never answered the ping: {attempt_result}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let result = result.unwrap();
    assert_eq!(result["source"], "ping");
    assert_eq!(result["online"], 1);
    assert_eq!(result["max"], 20);
    assert_eq!(result["sample"][0]["name"], "SmokeBot");
    assert_eq!(result["version"], "1.21.1");
    assert_eq!(result["motd"], "A fake server");
    assert!(result["latencyMs"].as_u64().is_some());

    client
        .request(
            methods::SERVER_STOP,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("stop accepted");
    wait_list_state(&mut client, "test", "stopped", Duration::from_secs(30)).await;

    // A stopped server does not answer: an empty room, never an error.
    let result = client
        .request(methods::PLAYERS_LIST, json!({"serverId": "test"}))
        .await
        .expect("players ping after stop");
    assert!(result["online"].is_null());
    assert_eq!(result["sample"].as_array().unwrap().len(), 0);

    // Unregistered → the usual typed miss.
    let error = client
        .request(methods::PLAYERS_LIST, json!({"serverId": "ghost"}))
        .await
        .expect_err("ghost");
    assert_eq!(error["code"], "SERVER_NOT_FOUND");
}
