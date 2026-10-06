//! Performance-budget suite (PERFORMANCE-BUDGETS.md): every budget is a
//! test, run by the nightly CI job — `#[ignore]`d here so the normal
//! `cargo test --workspace` stays fast and hermetic.
//!
//! ```sh
//! cargo test -p zamind --test perf -- --ignored --nocapture
//! ```
//!
//! These tests measure the real pipeline: the real daemon, the real IPC,
//! the real fake-mc-server flood. They are generous with variance where
//! CI runners are noisy (scheduling, shared hardware) but they assert the
//! published numbers, not vibes. A regression is a fix or a written
//! budget negotiation — same rules as the document says.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use serde_json::json;

mod common;

use common::{
    connect_daemon, make_server_root, register_server, scoped_dir, spawn_daemon, start_server,
    wait_for_state,
};
use zamin_protocol::envelope::IncomingMessage;
use zamin_protocol::methods;
use zamin_protocol::server::ServerState;
use zamin_protocol::streams::{StreamNotification, StreamPayload};

/// Percentile over a sorted sample (nearest-rank).
fn percentile(samples: &mut [u128], p: f64) -> u128 {
    samples.sort_unstable();
    let index = (((samples.len() as f64) * p) as usize).clamp(1, samples.len()) - 1;
    samples[index]
}

/// Daemon RSS in bytes, from /proc (Linux only — the budget doc's
/// reference platform; other lanes skip the memory assertions).
#[cfg(target_os = "linux")]
fn rss_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace()
        .nth(1)
        .and_then(|kb| kb.parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance budget: run with --ignored (nightly CI)"]
async fn ipc_round_trip_p50_under_1ms_p99_under_5ms() {
    let data_dir = scoped_dir("perf-ipc");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("perf-ipc");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    // Warm up: connections, allocator, first-touch paths.
    for _ in 0..100 {
        client
            .request(methods::DAEMON_STATUS, json!({}))
            .await
            .expect("warmup");
    }

    let samples_n = 2_000;
    let mut latencies = Vec::with_capacity(samples_n);
    for _ in 0..samples_n {
        let start = Instant::now();
        client
            .request(methods::DAEMON_STATUS, json!({}))
            .await
            .expect("status");
        latencies.push(start.elapsed().as_micros());
    }

    let p50_us = percentile(&mut latencies, 0.50);
    let p99_us = percentile(&mut latencies, 0.99);
    println!("IPC round trip: p50 = {p50_us} µs, p99 = {p99_us} µs (n = {samples_n})");
    assert!(p50_us < 1_000, "p50 {p50_us} µs exceeds the 1 ms budget");
    assert!(p99_us < 5_000, "p99 {p99_us} µs exceeds the 5 ms budget");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance budget: run with --ignored (nightly CI)"]
async fn cold_start_accepts_connections_under_a_second() {
    let mut starts = Vec::new();
    for run in 0..5 {
        let data_dir = scoped_dir(&format!("perf-cold-{run}"));
        let endpoint = zamin_ipc::Endpoint::unique_for_test(&format!("perf-cold-{run}"));
        let start = Instant::now();
        let _daemon = spawn_daemon(&data_dir, &endpoint);
        let mut client = connect_daemon(&endpoint).await;
        // The handshake is the "accepting and speaking the protocol" mark.
        client
            .request(methods::DAEMON_STATUS, json!({}))
            .await
            .expect("hello");
        starts.push(start.elapsed().as_millis());
    }

    let p50_ms = percentile(&mut starts, 0.50);
    let max_ms = *starts.iter().max().unwrap();
    println!("Cold start → accepting: p50 = {p50_ms} ms, max = {max_ms} ms (5 runs)");
    assert!(
        p50_ms < 500,
        "cold start p50 {p50_ms} ms exceeds the 500 ms budget"
    );
    assert!(
        max_ms < 1_000,
        "cold start max {max_ms} ms exceeds the 1 s p95 budget"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance budget: run with --ignored (nightly CI)"]
async fn log_ingestion_sustains_20k_lines_per_second() {
    let data_dir = scoped_dir("perf-flood");
    let root = make_server_root("root-perf-flood");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("perf-flood");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "flood", &root).await;

    // The fake server's unbounded flood produces lines as fast as the
    // process can emit them, so the measured rate is the daemon's real
    // pipeline capacity. The configured port gives
    // the supervisor a deterministic startup signal next to the flooded
    // log (port_is_listening), like a real chatty server with a port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    std::fs::write(
        root.join("server.properties"),
        format!("server-port={port}\n"),
    )
    .unwrap();
    common::write_server_config(
        &data_dir,
        "flood",
        &format!("port = {port}\nextraJvmArgs = [\"--flood-unbounded\"]\n"),
    );
    common::subscribe_events(&mut client, None).await;
    start_server(&mut client, "flood")
        .await
        .expect("start accepted");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(30))
        .await
        .expect("flood server reaches running");

    client
        .request(
            methods::STREAMS_SUBSCRIBE,
            json!({"stream": "logs", "serverId": "flood"}),
        )
        .await
        .expect("subscribe");

    // Count delivered lines over a 10 s window. The daemon's status stays
    // sampled from the same connection's peer: a stalled pipeline shows up
    // as probe latency, a stalled delivery as a low rate.
    let window = Duration::from_secs(10);
    let started = Instant::now();
    let mut lines: u64 = 0;
    while started.elapsed() < window {
        let remaining = window - started.elapsed();
        let Ok(frame) = tokio::time::timeout(
            remaining.min(Duration::from_millis(250)),
            client.connection.recv(),
        )
        .await
        else {
            continue;
        };
        let frame = frame.expect("open").expect("frame");
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&frame) else {
            continue;
        };
        let Some(IncomingMessage::Notification(note)) = IncomingMessage::parse(&value) else {
            continue;
        };
        let Ok(note) = serde_json::from_value::<StreamNotification>(note.params.expect("params"))
        else {
            continue;
        };
        if let StreamPayload::Logs { batch } = note.payload {
            lines += batch.len() as u64;
        }
    }

    let seconds = started.elapsed().as_secs_f64();
    let rate = lines as f64 / seconds;
    println!("Ingestion: {lines} lines in {seconds:.1} s = {rate:.0} lines/s");
    assert!(
        rate >= 20_000.0,
        "ingestion {rate:.0} lines/s is under the 20,000 lines/s budget"
    );

    // Memory stays bounded under sustained load (5 fake servers @ 1k
    // lines/s fits in 150 MB; one flood at 30k/s must fit there too).
    #[cfg(target_os = "linux")]
    {
        let daemon_pid = daemon_pid_of(&endpoint);
        if let Some(rss) = daemon_pid.and_then(rss_bytes) {
            println!("Daemon RSS under flood: {} MiB", rss / 1024 / 1024);
            assert!(
                rss < 150 * 1024 * 1024,
                "RSS {rss} exceeds the 150 MiB budget"
            );
        }
    }

    stop_server_gracefully(&mut client, "flood").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance budget: run with --ignored (nightly CI)"]
async fn events_deliver_within_20ms_at_p99() {
    let data_dir = scoped_dir("perf-events");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("perf-events");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    common::subscribe_events(&mut client, None).await;

    // Register servers one at a time; the registration's state_changed
    // event is the delivery-latency sample.
    let mut latencies = Vec::new();
    for i in 0..20 {
        let root = make_server_root(&format!("root-perf-events-{i}"));
        let start = Instant::now();
        register_server(&mut client, &format!("evt{i}"), &root).await;
        wait_for_state(&mut client, ServerState::NotRunning, Duration::from_secs(5))
            .await
            .expect("event arrives");
        latencies.push(start.elapsed().as_micros());
    }

    let p99_us = percentile(&mut latencies, 0.99);
    println!(
        "State change → event delivery: p99 = {p99_us} µs (n = {})",
        latencies.len()
    );
    assert!(
        p99_us < 20_000,
        "event delivery p99 {p99_us} µs exceeds the 20 ms budget"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance budget: run with --ignored (nightly CI)"]
async fn directory_listing_of_20k_entries_under_250ms() {
    let data_dir = scoped_dir("perf-listing");
    let root = make_server_root("root-perf-listing");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("perf-listing");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "big", &root).await;

    let big_dir = root.join("huge");
    std::fs::create_dir_all(&big_dir).unwrap();
    for i in 0..20_000 {
        std::fs::write(big_dir.join(format!("entry-{i:05}.txt")), "x").unwrap();
    }

    // Warm-up read, then sample.
    let _ = client
        .request(
            methods::FILES_LIST,
            json!({"serverId": "big", "path": "huge", "offset": 0, "limit": 1}),
        )
        .await
        .expect("warm listing");

    let mut latencies = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let listed = client
            .request(
                methods::FILES_LIST,
                json!({"serverId": "big", "path": "huge", "offset": 0, "limit": 1}),
            )
            .await
            .expect("listing");
        latencies.push(start.elapsed().as_micros());
        assert_eq!(listed["total"], 20_000);
    }

    let p95_us = percentile(&mut latencies, 0.95);
    println!("20k-entry listing: p95 = {p95_us} µs (5 samples)");
    assert!(
        p95_us < 250_000,
        "listing p95 {p95_us} µs exceeds the 250 ms budget"
    );
}

// --- local helpers over the common harness ---

async fn stop_server_gracefully(client: &mut common::Client, server_id: &str) {
    let _ = client
        .request(
            methods::SERVER_STOP,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": server_id}),
        )
        .await;
}

#[cfg(target_os = "linux")]
fn daemon_pid_of(endpoint: &zamin_ipc::Endpoint) -> Option<u32> {
    // The e2e harness spawns daemons as children of this test process;
    // find the zamind whose cmdline names this endpoint.
    let needle = match endpoint {
        zamin_ipc::Endpoint::UnixSocket(path) => path.to_string_lossy().into_owned(),
        zamin_ipc::Endpoint::WindowsPipe(name) => name.clone(),
    };
    let entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().parse::<u32>().ok();
        let Some(pid) = pid else { continue };
        let Ok(cmdline) = std::fs::read_to_string(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        if cmdline.contains("zamind") && cmdline.contains(&needle) {
            return Some(pid);
        }
    }
    None
}
