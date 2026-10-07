//! `zamin publish` end to end (ADR-0017): the real binary drives the real
//! daemon through the whole founder story — providers listed, config
//! authored with rule syntax, preview showing the diff and the fake
//! DiscordSRV token, the gate refusing, the §46 review clearing it, the
//! run landing the package, the state showing the receipt, and the diff
//! honestly resetting to "no changes".

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::atomic::{AtomicU32, Ordering};

use common::Harness;

fn unique_endpoint_tag(name: &str) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    format!("cli-publish-{}-{}", name, N.fetch_add(1, Ordering::Relaxed))
}

/// A believable fake Discord bot token: three base64url dot-separated
/// segments. It is fake; the shape is the point.
const FAKE_TOKEN: &str =
    "MTE0MTQxNDE0MTQxNDE0MTQxNA.GhUiWh.SFLQuN8SxjX0COoNUbMQhPdwOOms0TbYmGo5Qu4";

#[test]
fn cli_drives_publish_end_to_end() {
    let harness = Harness::spawn(&unique_endpoint_tag("verbs"));
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);

    // Seed a server tree: two clean plugin files and a DiscordSRV config
    // that carries a bot-token-shaped string.
    let root = &harness.root;
    std::fs::create_dir_all(root.join("plugins/TAB")).expect("tabs dir");
    std::fs::create_dir_all(root.join("plugins/DiscordSRV")).expect("dsrv dir");
    std::fs::write(root.join("server.properties"), "motd=Hello\n").expect("props");
    std::fs::write(
        root.join("plugins/TAB/config.yml"),
        "tablist:\n  enabled: true\n",
    )
    .expect("tab config");
    std::fs::write(
        root.join("plugins/DiscordSRV/config.yml"),
        format!("BotToken: \"{FAKE_TOKEN}\"\n"),
    )
    .expect("dsrv config");

    // Providers: both honest built-ins are named, no credential needed.
    let providers = harness.zamin(&["publish", "providers"]);
    assert!(providers.status.success());
    let providers_text = String::from_utf8_lossy(&providers.stdout);
    assert!(providers_text.contains("archive"), "{providers_text}");
    assert!(providers_text.contains("local-dir"), "{providers_text}");

    // Config: rule syntax, a partial set persists the rest.
    let configured = harness.zamin(&[
        "publish",
        "config",
        "demo",
        "--title",
        "Box Demo",
        "--version",
        "1.0.0",
        "--changelog",
        "first cut",
        "--include",
        "folder:plugins",
        "--include",
        "file:server.properties",
    ]);
    assert!(
        configured.status.success(),
        "{}",
        String::from_utf8_lossy(&configured.stderr)
    );
    let shown = harness.zamin(&["publish", "show", "demo"]);
    assert!(shown.status.success());
    let shown_text = String::from_utf8_lossy(&shown.stdout);
    assert!(shown_text.contains("folder:plugins"), "{shown_text}");
    assert!(shown_text.contains("Box Demo"));

    // Rule syntax typos die before any connection carries them.
    let bad_rule = harness.zamin(&["publish", "config", "demo", "--include", "nonsense"]);
    assert!(
        !bad_rule.status.success(),
        "a rule without a kind is a usage error"
    );

    // Preview: the diff says everything is new; the scan catches the
    // token with a redacted excerpt.
    let preview = harness.zamin(&["publish", "preview", "demo"]);
    assert!(preview.status.success());
    let preview_text = String::from_utf8_lossy(&preview.stdout);
    assert!(preview_text.contains("3 file(s) changed"), "{preview_text}");
    assert!(preview_text.contains("discord-bot-token"), "{preview_text}");
    assert!(preview_text.contains("critical"), "{preview_text}");
    assert!(
        !preview_text.contains(FAKE_TOKEN),
        "the token itself never reaches the operator's terminal"
    );
    assert!(preview_text.contains("block the publish"), "{preview_text}");

    // The gate: run refuses.
    let refused = harness.zamin(&["publish", "run", "demo"]);
    assert!(!refused.status.success(), "the gate must refuse");
    let refusal = String::from_utf8_lossy(&refused.stderr);
    assert!(refusal.contains("PUBLISH_SECRETS_DETECTED"), "{refusal}");

    // §46: review the finding as a false positive.
    let reviewed = harness.zamin(&[
        "publish",
        "review",
        "demo",
        "plugins/DiscordSRV/config.yml",
        "discord-bot-token",
    ]);
    assert!(
        reviewed.status.success(),
        "{}",
        String::from_utf8_lossy(&reviewed.stderr)
    );
    let reviewed_text = String::from_utf8_lossy(&reviewed.stdout);
    assert!(reviewed_text.contains("[reviewed]"), "{reviewed_text}");
    assert!(
        reviewed_text.contains("1 finding(s) still block"),
        "{reviewed_text}"
    );

    // The second detector on the same file gets its own review.
    let reviewed = harness.zamin(&[
        "publish",
        "review",
        "demo",
        "plugins/DiscordSRV/config.yml",
        "config-secret-key",
    ]);
    assert!(
        reviewed.status.success(),
        "{}",
        String::from_utf8_lossy(&reviewed.stderr)
    );
    let reviewed_text = String::from_utf8_lossy(&reviewed.stdout);
    assert!(
        reviewed_text.contains("0 finding(s) still block"),
        "{reviewed_text}"
    );

    // Run: the job publishes through the archive provider.
    let run = harness.zamin(&["publish", "run", "demo"]);
    assert!(
        run.status.success(),
        "{} {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).contains("Published."));

    // State: the receipt exists, the package is on disk.
    let state = harness.zamin(&["publish", "state", "demo"]);
    assert!(state.status.success());
    let state_text = String::from_utf8_lossy(&state.stdout);
    assert!(state_text.contains("archive"), "{state_text}");
    assert!(state_text.contains("on disk   yes"), "{state_text}");

    // The §42 diff resets honestly.
    let after = harness.zamin(&["publish", "preview", "demo"]);
    let after_text = String::from_utf8_lossy(&after.stdout);
    assert!(
        after_text.contains("No changes since the last publication"),
        "{after_text}"
    );
}

#[test]
fn cli_refuses_to_publish_an_empty_selection() {
    let harness = Harness::spawn(&unique_endpoint_tag("empty"));
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);

    // §41 is structural: with no include rules, run refuses with the
    // typed code — it never "packages everything" as a convenience.
    let refused = harness.zamin(&["publish", "run", "demo"]);
    assert!(!refused.status.success());
    let refusal = String::from_utf8_lossy(&refused.stderr);
    assert!(refusal.contains("PUBLISH_NOTHING_SELECTED"), "{refusal}");
}

#[test]
fn cli_local_dir_provider_lands_the_package_and_receipt() {
    let harness = Harness::spawn(&unique_endpoint_tag("localdir"));
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);
    std::fs::create_dir_all(harness.root.join("plugins/TAB")).expect("tabs dir");
    std::fs::write(
        harness.root.join("plugins/TAB/config.yml"),
        "tablist:\n  enabled: true\n",
    )
    .expect("tab config");

    let out_dir = common::scoped_dir("cli-publish-out");
    let configured = harness.zamin(&[
        "publish",
        "config",
        "demo",
        "--provider",
        "local-dir",
        "--setting",
        &format!("outDir={}", out_dir.to_string_lossy()),
        "--include",
        "folder:plugins",
    ]);
    assert!(
        configured.status.success(),
        "{}",
        String::from_utf8_lossy(&configured.stderr)
    );

    let run = harness.zamin(&["publish", "run", "demo"]);
    assert!(
        run.status.success(),
        "{} {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    // The package and its receipt landed in the operator's folder.
    assert!(out_dir.join("package.zip").is_file());
    assert!(out_dir.join("package.receipt.json").is_file());
}
