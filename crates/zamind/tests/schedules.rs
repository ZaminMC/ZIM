//! Schedules e2e (ADR-0014): the daemon runs the clock. The CRUD surface
//! over the real protocol (validation at the edge, typed not-found,
//! idempotent-authoring refusals), and the clock itself: a due interval
//! fires through the engine's ordinary paths (a console line reaches the
//! server, a backup lands as a real job), a disabled schedule never
//! fires, and lastFired is the only memory the clock keeps.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use serde_json::{json, Value};
use zamin_protocol::methods;

use common::{
    connect_daemon, make_server_root, poll_logs, register_server, scoped_dir, spawn_daemon,
    start_server, subscribe_events, wait_for_state, write_server_config, Client,
};

const LONG: Duration = Duration::from_secs(60);

fn request_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn create_params(server_id: &str, name: &str, spec: Value, action: Value, enabled: bool) -> Value {
    json!({
        "requestId": request_id(),
        "serverId": server_id,
        "name": name,
        "spec": spec,
        "action": action,
        "enabled": enabled,
    })
}

async fn list_schedules(client: &mut Client, server_id: &str) -> Value {
    client
        .request(
            methods::SCHEDULES_LIST,
            json!({ "requestId": request_id(), "serverId": server_id }),
        )
        .await
        .expect("schedules.list")
}

#[tokio::test]
async fn schedules_crud_validation_and_not_found() {
    let dir = scoped_dir("schedules-crud");
    let endpoint = zamin_ipc::Endpoint::UnixSocket(dir.join("d.sock"));
    let data_dir = dir.join("data");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("schedules-crud-root");
    register_server(&mut client, "demo", &root).await;

    // Empty to start: the absent store reads as no schedules.
    let listed = list_schedules(&mut client, "demo").await;
    assert_eq!(listed["schedules"].as_array().unwrap().len(), 0);

    // Create a daily restart. The name is trimmed at the edge, the view
    // carries a next-run hint, and enabled defaults through the wire.
    let created = client
        .request(
            methods::SCHEDULES_CREATE,
            create_params(
                "demo",
                "  nightly \n",
                json!({ "kind": "daily", "at": "04:30" }),
                json!({ "kind": "restart" }),
                true,
            ),
        )
        .await
        .expect("create daily");
    let first = created["schedule"].clone();
    assert_eq!(first["name"], "nightly");
    assert_eq!(first["spec"]["kind"], "daily");
    assert_eq!(first["spec"]["at"], "04:30");
    assert_eq!(first["enabled"], true);
    assert!(first["nextRunMs"].is_number(), "hint present: {first}");
    assert!(first["lastFiredMs"].is_null(), "never fired yet");
    let first_id = first["id"].as_str().unwrap().to_string();

    // A weekly command (disabled) and an interval backup.
    client
        .request(
            methods::SCHEDULES_CREATE,
            create_params(
                "demo",
                "weekend say",
                json!({ "kind": "weekly", "weekdays": ["sat", "sun"], "at": "09:00" }),
                json!({ "kind": "command", "line": "say hi" }),
                false,
            ),
        )
        .await
        .expect("create weekly");
    client
        .request(
            methods::SCHEDULES_CREATE,
            create_params(
                "demo",
                "world backups",
                json!({ "kind": "interval", "everySecs": 21600 }),
                json!({ "kind": "backup" }),
                true,
            ),
        )
        .await
        .expect("create interval");

    let listed = list_schedules(&mut client, "demo").await;
    let arr = listed["schedules"].as_array().unwrap();
    assert_eq!(arr.len(), 3, "three schedules stored: {listed}");
    assert_eq!(listed["serverId"], "demo");

    // Update: rename + re-enable; absent fields keep their values.
    let updated = client
        .request(
            methods::SCHEDULES_UPDATE,
            json!({
                "requestId": request_id(),
                "serverId": "demo",
                "scheduleId": first_id,
                "name": "renamed",
                "enabled": false,
            }),
        )
        .await
        .expect("update");
    assert_eq!(updated["schedule"]["name"], "renamed");
    assert_eq!(updated["schedule"]["enabled"], false);
    assert_eq!(updated["schedule"]["spec"]["at"], "04:30", "spec untouched");

    // An unknown id is a typed refusal, not a silent success.
    let err = client
        .request(
            methods::SCHEDULES_UPDATE,
            json!({
                "requestId": request_id(),
                "serverId": "demo",
                "scheduleId": "no-such-id",
                "enabled": true,
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(err["code"], "SCHEDULE_NOT_FOUND");

    // Validation happens at the edge: garbage never reaches the store.
    for (name, spec, action) in [
        (
            "bad hour",
            json!({ "kind": "daily", "at": "24:00" }),
            json!({ "kind": "restart" }),
        ),
        (
            "bad minute",
            json!({ "kind": "daily", "at": "04:3" }),
            json!({ "kind": "restart" }),
        ),
        (
            "empty weekdays",
            json!({ "kind": "weekly", "weekdays": [], "at": "04:00" }),
            json!({ "kind": "restart" }),
        ),
        (
            "bad weekday",
            json!({ "kind": "weekly", "weekdays": ["funday"], "at": "04:00" }),
            json!({ "kind": "restart" }),
        ),
        (
            "zero interval",
            json!({ "kind": "interval", "everySecs": 0 }),
            json!({ "kind": "restart" }),
        ),
        (
            "blank command",
            json!({ "kind": "interval", "everySecs": 600 }),
            json!({ "kind": "command", "line": "   " }),
        ),
    ] {
        let err = client
            .request(
                methods::SCHEDULES_CREATE,
                create_params("demo", name, spec, action, true),
            )
            .await
            .unwrap_err();
        assert_eq!(
            err["code"], "SCHEDULE_INVALID",
            "{name} must be refused: {err}"
        );
    }
    let err = client
        .request(
            methods::SCHEDULES_CREATE,
            create_params(
                "demo",
                "   ",
                json!({ "kind": "daily", "at": "04:00" }),
                json!({ "kind": "restart" }),
                true,
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(err["code"], "SCHEDULE_INVALID", "blank name");

    // Delete removes exactly once.
    client
        .request(
            methods::SCHEDULES_DELETE,
            json!({ "requestId": request_id(), "serverId": "demo", "scheduleId": first_id }),
        )
        .await
        .expect("delete");
    let err = client
        .request(
            methods::SCHEDULES_DELETE,
            json!({ "requestId": request_id(), "serverId": "demo", "scheduleId": first_id }),
        )
        .await
        .unwrap_err();
    assert_eq!(err["code"], "SCHEDULE_NOT_FOUND");

    // The registry owns identity: an unregistered server is not found.
    let err = client
        .request(
            methods::SCHEDULES_LIST,
            json!({ "requestId": request_id(), "serverId": "ghost" }),
        )
        .await
        .unwrap_err();
    assert_eq!(err["code"], "SERVER_NOT_FOUND");

    // The store survives the round trip to disk (write → read-back).
    let listed = list_schedules(&mut client, "demo").await;
    let arr = listed["schedules"].as_array().unwrap();
    assert_eq!(arr.len(), 2, "one deleted, two remain");
}

#[tokio::test]
async fn the_clock_fires_due_schedules_and_skips_disabled() {
    let dir = scoped_dir("schedules-fire");
    let endpoint = zamin_ipc::Endpoint::UnixSocket(dir.join("d.sock"));
    let data_dir = dir.join("data");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("schedules-fire-root");
    register_server(&mut client, "test", &root).await;
    write_server_config(&data_dir, "test", "port = 0\n");
    subscribe_events(&mut client, Some("test")).await;

    start_server(&mut client, "test")
        .await
        .expect("start accepted");
    wait_for_state(
        &mut client,
        zamin_protocol::server::ServerState::Running,
        Duration::from_secs(15),
    )
    .await
    .expect("server reaches running");

    // A second subscription for the logs stream: the scheduled command's
    // round trip is proven by a line on the console.
    client
        .request(
            methods::STREAMS_SUBSCRIBE,
            json!({ "requestId": request_id(), "stream": "logs", "serverId": "test" }),
        )
        .await
        .expect("subscribe logs");

    // Two enabled schedules due every two seconds (the wire floor — the
    // panel nudges operators to 300+), and their disabled twin.
    client
        .request(
            methods::SCHEDULES_CREATE,
            create_params(
                "test",
                "clock hello",
                json!({ "kind": "interval", "everySecs": 2 }),
                json!({ "kind": "command", "line": "say scheduled hello" }),
                true,
            ),
        )
        .await
        .expect("create command schedule");
    client
        .request(
            methods::SCHEDULES_CREATE,
            create_params(
                "test",
                "clock backup",
                json!({ "kind": "interval", "everySecs": 2 }),
                json!({ "kind": "backup" }),
                true,
            ),
        )
        .await
        .expect("create backup schedule");
    client
        .request(
            methods::SCHEDULES_CREATE,
            create_params(
                "test",
                "sleeping twin",
                json!({ "kind": "interval", "everySecs": 2 }),
                json!({ "kind": "command", "line": "say never" }),
                false,
            ),
        )
        .await
        .expect("create disabled schedule");

    // The tick runs every 15 s; both enabled schedules must have fired
    // within two ticks. lastFiredMs is the clock's only memory.
    let deadline = std::time::Instant::now() + LONG;
    let mut final_listed = Value::Null;
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "clock never fired: {final_listed}"
        );
        let listed = list_schedules(&mut client, "test").await;
        let arr = listed["schedules"].as_array().unwrap();
        let hello_fired = arr.iter().find(|s| s["name"] == "clock hello").unwrap();
        let backup_fired = arr.iter().find(|s| s["name"] == "clock backup").unwrap();
        if hello_fired["lastFiredMs"].is_number() && backup_fired["lastFiredMs"].is_number() {
            final_listed = listed;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let arr = final_listed["schedules"].as_array().unwrap();
    let sleeper = arr.iter().find(|s| s["name"] == "sleeping twin").unwrap();
    assert!(
        sleeper["lastFiredMs"].is_null(),
        "a disabled schedule never fires: {sleeper}"
    );

    // The scheduled command reached the server's console: the line rides
    // the live logs stream like any operator-typed one.
    let saw_it = poll_logs(&mut client, Duration::from_secs(10), |line| {
        line.contains("scheduled hello")
    })
    .await;
    assert!(saw_it, "the scheduled command never reached the console");

    // The scheduled backup is a real job, run through the ordinary path.
    // It starts in the same tick as the command, so give it its save
    // window: poll until a succeeded backup.create shows up.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let backup_job = loop {
        assert!(
            std::time::Instant::now() < deadline,
            "the scheduled backup never completed"
        );
        let jobs = client
            .request(methods::JOBS_LIST, json!({ "requestId": request_id() }))
            .await
            .expect("jobs.list");
        let found = jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["kind"] == "backup.create" && j["state"] == "succeeded")
            .cloned();
        if let Some(job) = found {
            break job;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    assert_eq!(
        backup_job["serverId"], "test",
        "the backup ran for the scheduled server"
    );
}
