//! End-to-end daemon test: spawn `zamind`, drive it over the real local
//! transport with the real protocol, and run a fake-mc-server through the
//! full lifecycle — register, start, run, console command, stop. This is
//! the Phase 1 acceptance harness (TESTING.md lifecycle matrix).
//!
//! The shared harness in `common` owns the daemon process, the protocol
//! client, and the event/log waits; this file keeps the acceptance
//! scenario itself.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use serde_json::json;

use zamin_protocol::methods;
use zamin_protocol::server::ServerState;

use common::{
    connect_daemon, make_server_root, poll_logs, register_server, scoped_dir, spawn_daemon,
    start_server, subscribe_events, wait_for_state, write_server_config,
};

#[tokio::test(flavor = "multi_thread")]
async fn full_lifecycle_over_the_wire() {
    let data_dir = scoped_dir("data");
    let server_root = make_server_root("server");
    // Per-server config: point the "java" at the fake server binary, which
    // speaks both the inspection probe and the lifecycle argv.
    write_server_config(&data_dir, "test", "");

    // Start the daemon on a test endpoint, connect, and say hello (the
    // handshake and its asserts live in `connect_daemon`).
    let endpoint = zamin_ipc::Endpoint::unique_for_test("e2e");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    // Register the fake server.
    let registered = register_server(&mut client, "test", &server_root).await;
    assert_eq!(registered["server"]["serverId"], "test");
    assert_eq!(registered["server"]["state"], "not-running");

    // Subscribe to lifecycle events BEFORE starting: subscriptions begin at
    // "now", so this is how a client observes the transitions.
    subscribe_events(&mut client, Some("test")).await;

    // Start: accepted as starting, becomes running via startup validation.
    let started = start_server(&mut client, "test")
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
