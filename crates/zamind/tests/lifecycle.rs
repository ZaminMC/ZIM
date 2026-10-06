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
