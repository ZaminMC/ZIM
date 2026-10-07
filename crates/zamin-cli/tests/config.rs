//! `zamin config` & `zamin network` end to end (ADR-0019): the real binary
//! drives the real daemon — the effective view with provenance words, the
//! tri-state set (absent keeps, --clear-<field> drops to the global
//! default), the refusals with the field named, and the network probe
//! with the properties authority, the bind-test, and conflicts.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::atomic::{AtomicU32, Ordering};

use common::Harness;

fn unique_endpoint_tag(name: &str) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    format!("cli-config-{}-{}", name, N.fetch_add(1, Ordering::Relaxed))
}

#[test]
fn cli_config_show_set_and_clear_round_trip() {
    let harness = Harness::spawn(&unique_endpoint_tag("verbs"));
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);

    // Show: built-in defaults, provenance global, the composed command.
    let shown = harness.zamin(&["config", "show", "demo"]);
    assert!(
        shown.status.success(),
        "{}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let text = String::from_utf8_lossy(&shown.stdout);
    assert!(text.contains("Server: demo ("), "{text}");
    assert!(text.contains("[global]"), "{text}");
    assert!(text.contains("server.jar"), "{text}");
    assert!(
        text.contains("nogui"),
        "the composed start command is visible: {text}"
    );

    // Set: overrides land, provenance flips to custom, the composed
    // command shows the new heap.
    let set = harness.zamin(&[
        "config",
        "set",
        "demo",
        "--port",
        "25566",
        "--max-memory-mb",
        "2048",
        // A value that starts with '-' must ride the equals form.
        "--jvm-arg=-XX:+UseG1GC",
        "--display-name",
        "Box Demo",
    ]);
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let text = String::from_utf8_lossy(&set.stdout);
    assert!(text.contains("[custom]"), "{text}");
    assert!(text.contains("-Xmx2048M"), "{text}");
    assert!(text.contains("-XX:+UseG1GC"), "{text}");
    assert!(text.contains("Box Demo"), "{text}");

    // The name reached the one registry (listings agree).
    let listed = harness.zamin(&["--json", "list"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("Box Demo"));

    // Keep semantics: a second set that names only the stop timeout
    // leaves the port override alone.
    let set = harness.zamin(&["config", "set", "demo", "--stop-timeout-secs", "45"]);
    assert!(set.status.success());
    let text = String::from_utf8_lossy(&set.stdout);
    assert!(text.contains("25566"), "the port override survived: {text}");

    // Clear: --clear-port drops the override (no global default here, so
    // the effective port goes back to unset).
    let cleared = harness.zamin(&["config", "set", "demo", "--clear-port"]);
    assert!(cleared.status.success());
    let text = String::from_utf8_lossy(&cleared.stdout);
    assert!(text.contains("(unset)"), "{text}");
    assert!(!text.contains("25566"), "{text}");

    // Refusals: nonsense is named by the daemon, exit code 1.
    let bad = harness.zamin(&["config", "set", "demo", "--max-memory-mb", "8"]);
    assert!(!bad.status.success());
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("CONFIG_INVALID"), "{err}");
    assert!(err.contains("maxMemoryMb"), "{err}");

    let bad = harness.zamin(&["config", "set", "demo"]);
    assert!(!bad.status.success(), "the empty set is refused");
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("nothing to set"), "{err}");

    // The panel/CLI never fight the daemon about the name: a blank name
    // is a typed refusal too.
    let bad = harness.zamin(&["config", "set", "demo", "--display-name", "   "]);
    assert!(!bad.status.success());
}

#[test]
fn cli_network_status_reports_probe_and_conflicts() {
    let harness = Harness::spawn(&unique_endpoint_tag("network"));

    // Two servers desiring the same port: each conflict list names the
    // other (§37's pre-start conflict check).
    harness.zamin_quiet(&["register", "alpha", harness.root.to_str().unwrap()]);
    let beta_root = common::scoped_dir("cli-config-beta-root");
    std::fs::write(beta_root.join("eula.txt"), "eula=true\n").unwrap();
    std::fs::write(beta_root.join("server.jar"), b"fake").unwrap();
    harness.zamin_quiet(&["register", "beta", beta_root.to_str().unwrap()]);

    // beta carries the boot authority in server.properties.
    std::fs::write(
        beta_root.join("server.properties"),
        "motd=hello\nserver-port=25580\nserver-ip=127.0.0.1\n",
    )
    .unwrap();

    // Hold a port for an honest in-use verdict.
    let listener = std::net::TcpListener::bind("0.0.0.0:25581").unwrap();

    harness.zamin_quiet(&["config", "set", "alpha", "--port", "25580"]);
    harness.zamin_quiet(&["config", "set", "beta", "--port", "25580"]);

    let status = harness.zamin(&["network", "status", "alpha"]);
    assert!(status.status.success());
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(text.contains("desired port          25580"), "{text}");
    assert!(text.contains("absent"), "alpha has no properties: {text}");
    assert!(text.contains("available right now"), "{text}");
    assert!(text.contains("beta"), "the conflict is named: {text}");

    let status = harness.zamin(&["network", "status", "beta"]);
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(
        text.contains("server.properties     25580"),
        "the boot authority is visible: {text}"
    );
    assert!(text.contains("127.0.0.1"), "{text}");
    assert!(text.contains("alpha"), "{text}");

    // The held port: the probe says in use, no conflicts.
    harness.zamin_quiet(&["config", "set", "alpha", "--port", "25581"]);
    let status = harness.zamin(&["network", "status", "alpha"]);
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(text.contains("in use right now"), "{text}");
    assert!(text.contains("conflicts             none"), "{text}");
    drop(listener);

    // --json is the scripting path: the raw protocol result (pretty).
    // The listener is dropped by now, so 25581 honestly answers available
    // again — the probe is a moment-in-time read, never a guarantee.
    let status = harness.zamin(&["--json", "network", "status", "alpha"]);
    assert!(status.status.success());
    let json_text = String::from_utf8_lossy(&status.stdout);
    assert!(json_text.contains("\"desiredPort\": 25581"), "{json_text}");
    assert!(json_text.contains("\"portAvailable\": true"), "{json_text}");
}
