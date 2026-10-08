//! Backup/restore e2e: the full job lifecycle over the real protocol —
//! create → events → list → restore → verify, the live save window, the
//! restore-while-running refusal, retention, and job queries.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use serde_json::{json, Value};
use zamin_protocol::envelope::IncomingMessage;
use zamin_protocol::methods;
use zamin_protocol::streams::{CoreEvent, StreamPayload};

use common::*;

const LONG: Duration = Duration::from_secs(30);

/// Read the events stream until the job completes; returns
/// (outcome-string, error-value, saw_started, saw_progress).
async fn wait_job_done(
    client: &mut Client,
    job_id: &str,
    timeout: Duration,
) -> (String, Option<Value>, bool, bool) {
    let target: uuid::Uuid = job_id.parse().expect("job id is a uuid");
    let deadline = std::time::Instant::now() + timeout;
    let mut saw_started = false;
    let mut saw_progress = false;
    while std::time::Instant::now() < deadline {
        // recv_notification replays the inbox first: the job.started
        // event races the create reply it answers, and the Client queues
        // that race's loser instead of dropping it.
        let value =
            match tokio::time::timeout(remaining_or_deadline(deadline), client.recv_notification())
                .await
            {
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
        let StreamPayload::Event { event } = params.payload else {
            continue;
        };
        match event {
            CoreEvent::JobStarted { job } => {
                if job.job_id == target {
                    saw_started = true;
                }
            }
            CoreEvent::JobProgress { .. } => saw_progress = true,
            CoreEvent::JobCompleted {
                job_id: done,
                outcome,
                error,
            } if done == target => {
                let error = error.as_ref().map(|e| serde_json::to_value(e).unwrap());
                return (format!("{outcome:?}"), error, saw_started, saw_progress);
            }
            _ => {}
        }
    }
    panic!("job {job_id} never completed within {timeout:?}");
}

fn seed_world(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("world/region")).unwrap();
    std::fs::write(root.join("world/level.dat"), b"world-v1").unwrap();
    std::fs::write(root.join("world/region/r.0.0.mca"), vec![3u8; 2048]).unwrap();
    std::fs::write(root.join("server.properties"), "motd=before\n").unwrap();
}

fn remaining_or_deadline(deadline: std::time::Instant) -> Duration {
    deadline.saturating_duration_since(std::time::Instant::now())
}

#[tokio::test]
async fn backup_create_list_restore_roundtrip() {
    let dir = scoped_dir("backups-roundtrip");
    let endpoint = common::endpoint_for(&dir);
    let data_dir = dir.join("data");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("backups-roundtrip-root");
    seed_world(&root);
    register_server(&mut client, "demo", &root).await;
    subscribe_events(&mut client, None).await;

    // Create: the reply carries a running job.
    let created = client
        .request(
            methods::BACKUP_CREATE,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
                "label": "before-map-reset",
            }),
        )
        .await
        .expect("backup.create");
    assert_eq!(created["kind"], "backup.create");
    assert_eq!(created["job"]["state"], "running");
    let job_id = created["job"]["jobId"].as_str().unwrap().to_owned();

    let (outcome, error, started, progress) = wait_job_done(&mut client, &job_id, LONG).await;
    assert_eq!(outcome, "Succeeded", "error: {error:?}");
    assert!(started, "job.started was published");
    assert!(progress, "job.progress was published");

    // The manifest reports a cold backup with real counts.
    let list = client
        .request(methods::BACKUPS_LIST, json!({"serverId": "demo"}))
        .await
        .expect("backups.list");
    let backups = list["backups"].as_array().unwrap();
    assert_eq!(backups.len(), 1, "{list}");
    assert_eq!(backups[0]["taken"], "cold");
    assert_eq!(backups[0]["label"], "before-map-reset");
    assert!(
        backups[0]["fileCount"].as_u64().unwrap() >= 4,
        "eula, jar, world files, properties: {list}"
    );

    // Tamper, then restore.
    let backup_id = backups[0]["backupId"].as_str().unwrap().to_owned();
    std::fs::write(root.join("server.properties"), "motd=TAMPERED\n").unwrap();
    std::fs::write(root.join("rogue.txt"), b"extra").unwrap();
    std::fs::remove_file(root.join("world/level.dat")).unwrap();

    let restored = client
        .request(
            methods::BACKUP_RESTORE,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
                "backupId": backup_id,
            }),
        )
        .await
        .expect("backup.restore");
    assert_eq!(restored["kind"], "backup.restore");
    let restore_job = restored["job"]["jobId"].as_str().unwrap().to_owned();
    let (outcome, error, _, _) = wait_job_done(&mut client, &restore_job, LONG).await;
    assert_eq!(outcome, "Succeeded", "error: {error:?}");

    assert_eq!(
        std::fs::read_to_string(root.join("server.properties")).unwrap(),
        "motd=before\n"
    );
    assert_eq!(
        std::fs::read(root.join("world/level.dat")).unwrap(),
        b"world-v1"
    );
    assert!(!root.join("rogue.txt").exists());
    // No restore workspace leftovers in the server root.
    assert!(!std::fs::read_dir(&root).unwrap().flatten().any(|e| e
        .file_name()
        .to_string_lossy()
        .starts_with(".zamin-restore-")));

    // jobs.list shows both finished records.
    let jobs = client
        .request(methods::JOBS_LIST, json!({}))
        .await
        .expect("jobs.list");
    let jobs = jobs["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2, "{jobs:?}");
    assert!(jobs
        .iter()
        .all(|j| j["state"] == "succeeded" || j["state"] == "failed" || j["state"] == "cancelled"));
}

#[tokio::test]
async fn restore_is_refused_while_running() {
    let dir = scoped_dir("backups-restore-running");
    let endpoint = common::endpoint_for(&dir);
    let data_dir = dir.join("data");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("backups-rr-root");
    write_server_config(&data_dir, "demo", "");
    register_server(&mut client, "demo", &root).await;
    subscribe_events(&mut client, None).await;

    // A first cold backup to restore later.
    let created = client
        .request(
            methods::BACKUP_CREATE,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
            }),
        )
        .await
        .expect("backup.create");
    let job_id = created["job"]["jobId"].as_str().unwrap().to_owned();
    let (outcome, error, _, _) = wait_job_done(&mut client, &job_id, LONG).await;
    assert_eq!(outcome, "Succeeded", "{error:?}");

    // Start the server, then refuse the restore.
    start_server(&mut client, "demo").await.expect("start");
    wait_for_state(
        &mut client,
        zamin_protocol::server::ServerState::Running,
        LONG,
    )
    .await
    .expect("running");

    let backup_id = client
        .request(methods::BACKUPS_LIST, json!({"serverId": "demo"}))
        .await
        .unwrap()["backups"][0]["backupId"]
        .as_str()
        .unwrap()
        .to_owned();
    let err = client
        .request(
            methods::BACKUP_RESTORE,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
                "backupId": backup_id,
            }),
        )
        .await
        .expect_err("restore must be refused");
    assert_eq!(err["code"], "SERVER_ALREADY_RUNNING", "{err}");

    // The live backup DOES work while running: the save window wraps it.
    let created = client
        .request(
            methods::BACKUP_CREATE,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
            }),
        )
        .await
        .expect("live backup.create");
    let job_id = created["job"]["jobId"].as_str().unwrap().to_owned();
    let (outcome, error, _, _) = wait_job_done(&mut client, &job_id, LONG).await;
    assert_eq!(outcome, "Succeeded", "{error:?}");
    let list = client
        .request(methods::BACKUPS_LIST, json!({"serverId": "demo"}))
        .await
        .unwrap();
    let latest = list["backups"][0].clone();
    assert_eq!(latest["taken"], "live", "{list}");

    // The save window left its marks in the server's own log. A logs
    // subscription is needed for that: subscribe, then poll.
    client
        .request(
            methods::STREAMS_SUBSCRIBE,
            json!({"stream": "logs", "serverId": "demo"}),
        )
        .await
        .expect("subscribe logs");
    assert!(
        poll_logs(&mut client, Duration::from_secs(5), |line| {
            line.contains("Turned off world auto-saving") || line.contains("Saved the game")
        })
        .await,
        "the save window never reached the server"
    );

    stop_server(&mut client, "demo").await;
    wait_list_state(&mut client, "demo", "stopped", LONG).await;

    // After a graceful stop the state is `stopped`, not `not-running` —
    // the restore must be ALLOWED there too (the browser smoke caught
    // exactly this: a strict not-running check bricked restores after a
    // normal stop).
    let restored = client
        .request(
            methods::BACKUP_RESTORE,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
                "backupId": backup_id,
            }),
        )
        .await
        .expect("restore after a graceful stop is allowed");
    let restore_job = restored["job"]["jobId"].as_str().unwrap().to_owned();
    let (outcome, error, _, _) = wait_job_done(&mut client, &restore_job, LONG).await;
    assert_eq!(outcome, "Succeeded", "{error:?}");
}

#[tokio::test]
async fn retention_prunes_to_configured_keep() {
    let dir = scoped_dir("backups-retention");
    let endpoint = common::endpoint_for(&dir);
    let data_dir = dir.join("data");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("backups-ret-root");
    write_server_config(&data_dir, "demo", "backupKeep = 2\n");
    register_server(&mut client, "demo", &root).await;
    subscribe_events(&mut client, None).await;

    for i in 0..3 {
        let created = client
            .request(
                methods::BACKUP_CREATE,
                json!({
                    "requestId": uuid::Uuid::now_v7().to_string(),
                    "serverId": "demo",
                    "label": format!("round-{i}"),
                }),
            )
            .await
            .expect("backup.create");
        let job_id = created["job"]["jobId"].as_str().unwrap().to_owned();
        let (outcome, error, _, _) = wait_job_done(&mut client, &job_id, LONG).await;
        assert_eq!(outcome, "Succeeded", "round {i}: {error:?}");
        // Retention runs after the create; give the janitor a beat.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let list = client
        .request(methods::BACKUPS_LIST, json!({"serverId": "demo"}))
        .await
        .expect("backups.list");
    let backups = list["backups"].as_array().unwrap();
    assert_eq!(backups.len(), 2, "retention keeps 2: {list}");
    // The newest two survive, oldest first order is reversed in the reply.
    assert_eq!(backups[0]["label"], "round-2");
    assert_eq!(backups[1]["label"], "round-1");
}

#[tokio::test]
async fn unknown_job_and_restore_target_are_typed_errors() {
    let dir = scoped_dir("backups-typed-errors");
    let endpoint = common::endpoint_for(&dir);
    let data_dir = dir.join("data");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("backups-te-root");
    register_server(&mut client, "demo", &root).await;

    let err = client
        .request(
            methods::JOBS_CANCEL,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "jobId": uuid::Uuid::now_v7().to_string(),
            }),
        )
        .await
        .expect_err("unknown job");
    assert_eq!(err["code"], "JOB_NOT_FOUND", "{err}");

    let err = client
        .request(
            methods::JOBS_GET,
            json!({"jobId": uuid::Uuid::now_v7().to_string()}),
        )
        .await
        .expect_err("unknown job");
    assert_eq!(err["code"], "JOB_NOT_FOUND", "{err}");

    let err = client
        .request(
            methods::BACKUP_RESTORE,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "demo",
                "backupId": uuid::Uuid::now_v7().to_string(),
            }),
        )
        .await
        .expect_err("unknown backup");
    assert_eq!(err["code"], "FS_NOT_FOUND", "{err}");
}

// Small local helpers on top of the common harness.
async fn stop_server(client: &mut Client, server_id: &str) {
    let _ = client
        .request(
            methods::SERVER_STOP,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": server_id,
            }),
        )
        .await;
}
