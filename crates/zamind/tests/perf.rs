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

use bytes::Bytes;
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

/// The burst budget (PERFORMANCE-BUDGETS.md): 50,000 lines/s with zero
/// unbounded memory growth; a slow subscriber sees `missed: N` and the
/// daemon does not stall. One connection stops reading while the flood
/// keeps publishing — the bounded subscriber queue fills, the hub
/// accumulates the drop count (ADR-0006: no silent loss, a marker leads
/// the catch-up) — while a second connection keeps probing
/// `daemon.status` to prove the pipeline never blocks on delivery.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance budget: run with --ignored (nightly CI)"]
async fn log_burst_slow_subscriber_gets_missed_marker_and_daemon_does_not_stall() {
    let data_dir = scoped_dir("perf-burst");
    let root = make_server_root("root-perf-burst");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("perf-burst");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

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
        "burst",
        &format!("port = {port}\nextraJvmArgs = [\"--flood-unbounded\"]\n"),
    );
    register_server(&mut client, "burst", &root).await;
    common::subscribe_events(&mut client, None).await;
    start_server(&mut client, "burst")
        .await
        .expect("start accepted");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(30))
        .await
        .expect("burst server reaches running");

    client
        .request(
            methods::STREAMS_SUBSCRIBE,
            json!({"stream": "logs", "serverId": "burst"}),
        )
        .await
        .expect("subscribe logs");

    // The stall: stop reading this connection entirely. The daemon keeps
    // ingesting; the bounded subscriber queue (1024 notifications) fills
    // and the hub starts counting drops. The pump publishes at 256-line
    // batches (~220 notifications/s at flood rates), so the queue
    // overflows well inside 14 s.
    let mut probe = connect_daemon(&endpoint).await;
    let stall = Duration::from_secs(14);
    let stalled_at = Instant::now();
    let mut probes = 0;
    let mut worst_probe_us = 0u128;
    while stalled_at.elapsed() < stall {
        let start = Instant::now();
        probe
            .request(methods::DAEMON_STATUS, json!({}))
            .await
            .expect("status during stall");
        worst_probe_us = worst_probe_us.max(start.elapsed().as_micros());
        probes += 1;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    println!("During {stall:?} stall: {probes} probes, worst round trip {worst_probe_us} µs");
    assert!(
        worst_probe_us < 1_000_000,
        "daemon stalled under a slow subscriber: worst probe {worst_probe_us} µs"
    );

    // Zero unbounded memory growth: the queue is bounded, so the peak is
    // bounded — sample it at the end of the stall.
    #[cfg(target_os = "linux")]
    {
        let daemon_pid = daemon_pid_of(&endpoint);
        if let Some(rss) = daemon_pid.and_then(rss_bytes) {
            println!(
                "Daemon RSS with stalled subscriber: {} MiB",
                rss / 1024 / 1024
            );
            assert!(
                rss < 150 * 1024 * 1024,
                "RSS {rss} under a stalled subscriber exceeds the 150 MiB budget"
            );
        }
    }

    // Resume: drain the backlog. The catch-up marker leads the first
    // publication that finds a free slot — it carries the accumulated
    // drop count. Backlog frames are Logs payloads; the marker is the
    // only frame shape containing "missed", so the scan pre-filters on
    // the raw bytes and parses only candidates.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut scanned = 0u64;
    let mut missed_total = 0u64;
    let mut found = false;
    while Instant::now() < deadline {
        let remaining = deadline - Instant::now();
        let frame = tokio::time::timeout(
            remaining.min(Duration::from_millis(500)),
            client.connection.recv(),
        )
        .await
        .expect("recv within deadline")
        .expect("open")
        .expect("frame");
        scanned += 1;
        if !frame.windows(6).any(|w| w == b"missed") {
            continue;
        }
        let value: serde_json::Value = serde_json::from_slice(&frame).expect("json");
        let Some(IncomingMessage::Notification(note)) = IncomingMessage::parse(&value) else {
            continue;
        };
        let Ok(note) = serde_json::from_value::<StreamNotification>(note.params.expect("params"))
        else {
            continue;
        };
        if let StreamPayload::Missed { missed } = note.payload {
            missed_total += missed;
            found = true;
            println!("Missed marker after the stall: {missed} dropped notifications");
            break;
        }
    }
    assert!(
        found,
        "no Missed marker after draining {scanned} frames — the queue never \
         overflowed (publication rate too low on this runner?)"
    );
    assert!(missed_total >= 1, "marker must carry a positive drop count");

    // Fresh measurement window over the still-running flood: the burst
    // rate and the batch shape (delivery is batched, never one message
    // per line — the point of the flush tick in the log pump).
    let window = Duration::from_secs(5);
    let started = Instant::now();
    let mut lines: u64 = 0;
    let mut notifications: u64 = 0;
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
            notifications += 1;
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    let rate = lines as f64 / seconds;
    let avg_batch = lines as f64 / notifications.max(1) as f64;
    println!(
        "Burst after catch-up: {rate:.0} lines/s, avg batch {avg_batch:.0} lines \
         over {notifications} notifications"
    );
    // The hard line everywhere is the sustained budget — burst conditions
    // must not degrade the pipeline below it. The 50k burst figure in
    // PERFORMANCE-BUDGETS.md is a reference-hardware number (mid-range
    // 2023 laptop); CI runners are slower and shared, so the nightly
    // records the measured rate and gates on the sustained line plus the
    // invariants (no stall, missed marker, bounded memory, batch shape).
    assert!(
        rate >= 20_000.0,
        "burst rate {rate:.0} lines/s is under the sustained 20,000 lines/s budget"
    );
    assert!(
        avg_batch >= 50.0,
        "avg batch {avg_batch:.0} lines — delivery is not batched (never one \
         message per line under load)"
    );

    stop_server_gracefully(&mut client, "burst").await;
}

/// Daemon RSS, idle: < 50 MB (reference platform only — /proc).
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance budget: run with --ignored (nightly CI)"]
async fn daemon_rss_idle_under_50mb() {
    let data_dir = scoped_dir("perf-rss-idle");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("perf-rss-idle");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    client
        .request(methods::DAEMON_STATUS, json!({}))
        .await
        .expect("hello");

    // Let one metrics-less idle tick pass and the allocator settle.
    tokio::time::sleep(Duration::from_secs(1)).await;

    let pid = daemon_pid_of(&endpoint).expect("daemon pid found");
    let rss = rss_bytes(pid).expect("VmRSS present");
    println!("Idle daemon RSS: {} MiB", rss / 1024 / 1024);
    assert!(
        rss < 50 * 1024 * 1024,
        "idle RSS {rss} exceeds the 50 MiB budget"
    );
}

/// The five-server shape: 5 fake servers each streaming 1k lines/s with
/// an active subscriber draining — total daemon RSS stays under 150 MB.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance budget: run with --ignored (nightly CI)"]
async fn five_servers_streaming_1k_lines_each_stay_under_150mb() {
    let data_dir = scoped_dir("perf-fleet");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("perf-fleet");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    common::subscribe_events(&mut client, None).await;

    const FLEET: usize = 5;
    let mut ports = Vec::with_capacity(FLEET);
    for _ in 0..FLEET {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        ports.push(listener.local_addr().unwrap().port());
        drop(listener);
    }
    for (i, port) in ports.iter().enumerate() {
        let id = format!("fleet{i}");
        let root = make_server_root(&format!("root-perf-fleet-{i}"));
        std::fs::write(
            root.join("server.properties"),
            format!("server-port={port}\n"),
        )
        .unwrap();
        common::write_server_config(
            &data_dir,
            &id,
            &format!("port = {port}\nextraJvmArgs = [\"--flood-stdout\", \"1000\"]\n"),
        );
        register_server(&mut client, &id, &root).await;
    }
    for i in 0..FLEET {
        let id = format!("fleet{i}");
        start_server(&mut client, &id)
            .await
            .expect("start accepted");
        wait_for_state(&mut client, ServerState::Running, Duration::from_secs(30))
            .await
            .unwrap_or_else(|| panic!("{id} reaches running"));
    }

    client
        .request(methods::STREAMS_SUBSCRIBE, json!({"stream": "logs"}))
        .await
        .expect("subscribe fleet logs");

    // Soak: drain like an active subscriber, then sample the peak.
    let soak = Duration::from_secs(6);
    let started = Instant::now();
    let mut lines: u64 = 0;
    while started.elapsed() < soak {
        let remaining = soak - started.elapsed();
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
    println!("Fleet soak: {lines} lines over {:.1} s", soak.as_secs_f64());
    // Sanity only: the producer paces itself with thread sleeps, so the
    // achievable aggregate varies with scheduler noise (measured ~4k/s on
    // a loaded 2-core box). The budget under test is RSS, not throughput
    // here — this line proves all five servers stream.
    assert!(
        lines >= 2_000 * soak.as_secs(),
        "fleet throughput {lines} lines in {:?} — servers are not streaming",
        soak
    );

    #[cfg(target_os = "linux")]
    {
        let daemon_pid = daemon_pid_of(&endpoint);
        if let Some(rss) = daemon_pid.and_then(rss_bytes) {
            println!(
                "Daemon RSS, 5 servers @ 1k lines/s: {} MiB",
                rss / 1024 / 1024
            );
            assert!(
                rss < 150 * 1024 * 1024,
                "RSS {rss} exceeds the 150 MiB five-server budget"
            );
        }
    }

    for i in 0..FLEET {
        stop_server_gracefully(&mut client, &format!("fleet{i}")).await;
    }
}

/// Terminal input echo, round trip via the daemon: send `server.stdin`,
/// wait for the server's reply line on the logs stream. p99 < 50 ms.
/// The reply's latency is quantized by the pump's flush tick — the tick
/// (10 ms) exists so this budget is satisfiable at all; see the comment
/// in `log pump` (actor.rs) and the negotiated wording in
/// PERFORMANCE-BUDGETS.md.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "performance budget: run with --ignored (nightly CI)"]
async fn terminal_echo_round_trip_p99_under_50ms() {
    let data_dir = scoped_dir("perf-echo");
    let root = make_server_root("root-perf-echo");
    // Point "java" at the fake server binary (no flood flags — the echo
    // path needs a quiet server). Without this the daemon falls back to
    // the system java, which cannot run the fake jar.
    common::write_server_config(&data_dir, "echo", "");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("perf-echo");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    register_server(&mut client, "echo", &root).await;
    common::subscribe_events(&mut client, None).await;
    start_server(&mut client, "echo")
        .await
        .expect("start accepted");
    wait_for_state(&mut client, ServerState::Running, Duration::from_secs(30))
        .await
        .expect("echo server reaches running");

    client
        .request(
            methods::STREAMS_SUBSCRIBE,
            json!({"stream": "logs", "serverId": "echo"}),
        )
        .await
        .expect("subscribe logs");

    const SAMPLES: usize = 100;
    let mut latencies = Vec::with_capacity(SAMPLES);
    for i in 0..SAMPLES {
        let marker = format!("echo-perf-{i}");
        let request = json!({
            "jsonrpc": "2.0",
            "id": 10_000 + i as u64,
            "method": methods::SERVER_STDIN,
            "params": {
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "echo",
                "line": marker,
            },
        });
        let start = Instant::now();
        client
            .connection
            .send(Bytes::from(serde_json::to_vec(&request).unwrap()))
            .await
            .expect("send stdin");
        // The fake server answers unknown commands with
        // "Unknown command: <line>" — that reply line is the echo.
        let needle = format!("Unknown command: {marker}");
        let deadline = start + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < deadline, "echo for {marker} never arrived");
            let frame = tokio::time::timeout(Duration::from_secs(5), client.connection.recv())
                .await
                .expect("recv")
                .expect("open")
                .expect("frame");
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&frame) else {
                continue;
            };
            let Some(IncomingMessage::Notification(note)) = IncomingMessage::parse(&value) else {
                continue; // the stdin request's own response, earlier replies
            };
            let Ok(note) =
                serde_json::from_value::<StreamNotification>(note.params.expect("params"))
            else {
                continue;
            };
            let StreamPayload::Logs { batch } = note.payload else {
                continue;
            };
            if batch.iter().any(|line| line.line.contains(&needle)) {
                latencies.push(start.elapsed().as_micros());
                break;
            }
        }
    }

    let p50_us = percentile(&mut latencies, 0.50);
    let p99_us = percentile(&mut latencies, 0.99);
    println!("Terminal echo round trip: p50 = {p50_us} µs, p99 = {p99_us} µs (n = {SAMPLES})");
    assert!(
        p99_us < 50_000,
        "echo p99 {p99_us} µs exceeds the 50 ms budget"
    );

    stop_server_gracefully(&mut client, "echo").await;
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
