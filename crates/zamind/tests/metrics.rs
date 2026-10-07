//! `metrics` stream + `metrics.range` end-to-end: the actor's 1 Hz sampler
//! (CPU from counter deltas, RSS from the OS, players from the live roster,
//! uptime) publishing through the hub to a real subscriber over the wire,
//! and the ring-backed history read. Everything measured, nothing faked
//! (ADR-0006: TPS is only ever set when actually measured — here, never).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use serde_json::{json, Value};
use zamin_protocol::methods;
use zamin_protocol::server::ServerState;

mod common;

use common::{
    connect_daemon, make_server_root, register_server, scoped_dir, spawn_daemon, start_server,
    subscribe_events, wait_for_state, write_server_config, Client,
};

/// Read the wire for `duration`, collecting metrics payloads. Notifications
/// and responses share one multiplexed connection, so responses are passed
/// through untouched.
async fn collect_metrics(client: &mut Client, duration: Duration) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + duration;
    let mut samples = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return samples;
        }
        let frame = match tokio::time::timeout(remaining, client.connection.recv()).await {
            Ok(Ok(Some(frame))) => frame,
            _ => return samples,
        };
        let value: Value = serde_json::from_slice(&frame).unwrap();
        if value["params"]["stream"] == "metrics" {
            samples.push(value["params"]["payload"]["sample"].clone());
        }
    }
}

async fn stdin(client: &mut Client, line: &str) {
    client
        .request(
            methods::SERVER_STDIN,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "test",
                "line": line,
            }),
        )
        .await
        .expect("stdin accepted");
}

#[tokio::test]
async fn metrics_flow_while_running_and_range_serves_the_ring() {
    let data_dir = scoped_dir("metrics-sampler");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("metrics-sampler");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    subscribe_events(&mut client, Some("test")).await;

    let root = make_server_root("metrics-sampler");
    register_server(&mut client, "test", &root).await;
    write_server_config(&data_dir, "test", "port = 0\n");

    start_server(&mut client, "test")
        .await
        .expect("start accepted");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(15))
        .await
        .expect("server reaches running");

    // Subscribe and watch ~3.2s of 1 Hz samples.
    client
        .request(
            methods::STREAMS_SUBSCRIBE,
            json!({"stream": "metrics", "serverId": "test"}),
        )
        .await
        .expect("subscribe metrics");
    let samples = collect_metrics(&mut client, Duration::from_millis(3200)).await;
    assert!(
        samples.len() >= 2,
        "expected at least 2 samples in 3.2s, got {}",
        samples.len()
    );

    // Monotonic timestamps; RSS measured on every platform we ship.
    let mut last_ts = 0i64;
    for (index, sample) in samples.iter().enumerate() {
        let ts = sample["tsMs"].as_i64().expect("tsMs present");
        assert!(ts > last_ts, "sample timestamps ascend");
        last_ts = ts;
        let rss = sample["rssBytes"].as_u64().expect("rss measured");
        assert!(rss > 1_000_000, "a live process uses more than 1 MB: {rss}");
        // The first sample has no counter base; every later one does.
        if index > 0 {
            let cpu = sample["cpuPercent"].as_f64().expect("cpu measured");
            assert!(cpu >= 0.0, "cpu percent is non-negative, got {cpu}");
        }
        let uptime = sample["uptimeMs"].as_i64().expect("uptime present");
        assert!(uptime >= 0, "uptime is non-negative, got {uptime}");
    }

    // The player count comes from the live roster, measured not guessed.
    stdin(&mut client, "join Steve").await;
    let samples = collect_metrics(&mut client, Duration::from_millis(2500)).await;
    assert!(
        samples
            .iter()
            .any(|sample| sample["players"].as_u64() == Some(1)),
        "a joined player shows up in the sample stream"
    );

    // A fresh subscription receives the ring's latest sample first
    // (e2e companion to the hub unit test — the opening delivery must
    // survive the wire).
    let last_seen_ts = last_ts;
    client
        .request(
            methods::STREAMS_SUBSCRIBE,
            json!({"stream": "metrics", "serverId": "test"}),
        )
        .await
        .expect("second metrics subscription");
    let opening = collect_metrics(&mut client, Duration::from_millis(1200)).await;
    assert!(
        opening
            .first()
            .and_then(|sample| sample["tsMs"].as_i64())
            .is_some_and(|ts| ts >= last_seen_ts),
        "the first delivery on a fresh subscription is the ring's latest \
         (ts {last_seen_ts}), got {:?}",
        opening.first()
    );

    // Stop the server; the ring survives the process.
    client
        .request(
            methods::SERVER_STOP,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("stop accepted");
    wait_for_state(&mut client, ServerState::Stopped, Duration::from_secs(15))
        .await
        .expect("server stops");

    // metrics.range: bounded, chronological, newest kept when trimmed.
    let reply: Value = client
        .request(
            methods::METRICS_RANGE,
            json!({"serverId": "test", "maxSamples": 2}),
        )
        .await
        .expect("metrics.range answers");
    let trimmed: Vec<Value> = reply["samples"].as_array().expect("samples array").to_vec();
    assert_eq!(trimmed.len(), 2, "maxSamples trims to the newest two");
    let ts: Vec<i64> = trimmed
        .iter()
        .map(|sample| sample["tsMs"].as_i64().expect("ts"))
        .collect();
    assert!(ts[0] < ts[1], "chronological, oldest first: {ts:?}");
    assert!(ts[1] >= last_seen_ts, "the newest samples are kept");

    let reply: Value = client
        .request(methods::METRICS_RANGE, json!({"serverId": "test"}))
        .await
        .expect("metrics.range default answers");
    let all = reply["samples"].as_array().expect("samples array").len();
    assert!(
        all >= 5,
        "the default window holds what was collected: {all}"
    );

    // An unknown server is a typed error, not an empty answer.
    let error = client
        .request(
            methods::METRICS_RANGE,
            json!({"serverId": "no-such-server", "maxSamples": 2}),
        )
        .await
        .expect_err("unknown server errors");
    assert!(
        error["data"]["code"].as_str() == Some("SERVER_NOT_FOUND")
            || error["code"].as_str() == Some("SERVER_NOT_FOUND"),
        "typed SERVER_NOT_FOUND, got {error}"
    );
}
