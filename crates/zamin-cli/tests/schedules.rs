//! `zamin schedules` end to end (ADR-0014): the real binary drives the
//! real daemon — add every when-shape (interval, daily, weekly) and every
//! then-shape (restart, backup, command), the list renders the clock's
//! memory, pause/resume flip the enabled bit, remove is a typed refusal
//! the second time, and a when-less add is a usage error before any
//! connection carries it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::atomic::{AtomicU32, Ordering};

use common::Harness;

/// A distinct endpoint per test: the tag is hashed with the pid, so two
/// tests sharing a tag share a socket — give each test its own.
fn unique_endpoint_tag(name: &str) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    format!(
        "cli-schedules-{}-{}",
        name,
        N.fetch_add(1, Ordering::Relaxed)
    )
}

#[test]
fn cli_drives_schedules_end_to_end() {
    let harness = Harness::spawn(&unique_endpoint_tag("verbs"));
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);

    // Empty list names the honest state.
    let empty = harness.zamin(&["schedules", "list", "demo"]);
    assert!(empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stdout).contains("No schedules"));

    // Add: daily restart (the default then).
    let added = harness.zamin(&[
        "schedules",
        "add",
        "demo",
        "--name",
        "nightly restart",
        "--at",
        "04:30",
    ]);
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    assert!(String::from_utf8_lossy(&added.stdout).contains("nightly restart"));

    // Add: interval backup and a weekly console line.
    assert!(harness
        .zamin(&[
            "schedules",
            "add",
            "demo",
            "--name",
            "hourly backup",
            "--every",
            "3600",
            "--backup",
        ])
        .status
        .success());
    assert!(harness
        .zamin(&[
            "schedules",
            "add",
            "demo",
            "--name",
            "weekend hello",
            "--weekdays",
            "sat,sun",
            "--at",
            "09:00",
            "--command",
            "say hi",
        ])
        .status
        .success());

    // JSON list: three schedules, the spec shapes intact, the next-run
    // hint present for the enabled ones.
    let list = harness.zamin_json(&["schedules", "list", "demo"]);
    let arr = list["schedules"].as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["spec"]["kind"], "daily");
    assert_eq!(arr[0]["spec"]["at"], "04:30");
    assert_eq!(arr[0]["action"]["kind"], "restart");
    assert_eq!(arr[1]["spec"]["kind"], "interval");
    assert_eq!(arr[1]["spec"]["everySecs"], 3600);
    assert_eq!(arr[1]["action"]["kind"], "backup");
    assert_eq!(arr[2]["spec"]["kind"], "weekly");
    assert_eq!(
        arr[2]["spec"]["weekdays"],
        serde_json::json!(["sat", "sun"])
    );
    assert_eq!(
        arr[2]["action"],
        serde_json::json!({ "kind": "command", "line": "say hi" })
    );
    assert!(arr[0]["nextRunMs"].is_number());
    assert!(arr[0]["lastFiredMs"].is_null(), "never fired yet");

    // The daemon refuses a bad HH:MM with the typed code.
    let refused = harness.zamin(&[
        "schedules",
        "add",
        "demo",
        "--name",
        "broken",
        "--at",
        "24:00",
    ]);
    assert_eq!(refused.status.code(), Some(1));
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(err.contains("SCHEDULE_INVALID"), "typed refusal: {err}");

    // A when-less add never reaches the daemon either.
    let usage = harness.zamin(&["schedules", "add", "demo", "--name", "no-when"]);
    assert_eq!(usage.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&usage.stderr).contains("pick a when"));

    // Pause and resume flip the enabled bit through the update verb.
    let id = arr[0]["id"].as_str().unwrap().to_owned();
    assert!(harness
        .zamin(&["schedules", "pause", "demo", &id])
        .status
        .success());
    let paused = harness.zamin_json(&["schedules", "list", "demo"]);
    assert_eq!(paused["schedules"][0]["enabled"], false);
    assert!(
        paused["schedules"][0]["nextRunMs"].is_null(),
        "paused: no hint"
    );
    assert!(harness
        .zamin(&["schedules", "resume", "demo", &id])
        .status
        .success());
    let resumed = harness.zamin_json(&["schedules", "list", "demo"]);
    assert_eq!(resumed["schedules"][0]["enabled"], true);

    // Remove: gone once, a typed refusal the second time.
    assert!(harness
        .zamin(&["schedules", "remove", "demo", &id])
        .status
        .success());
    let gone = harness.zamin_json(&["schedules", "list", "demo"]);
    assert_eq!(gone["schedules"].as_array().unwrap().len(), 2);
    let again = harness.zamin(&["schedules", "remove", "demo", &id]);
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("SCHEDULE_NOT_FOUND"));

    // JSON mode: the delete answers a script-friendly object.
    let other = gone["schedules"][0]["id"].as_str().unwrap().to_owned();
    let removed = harness.zamin_json(&["schedules", "remove", "demo", &other]);
    assert_eq!(removed["removed"], other);
}
