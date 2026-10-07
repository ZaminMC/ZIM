//! Publish e2e (founder §40–47, §74, ADR-0017): the provider list, the
//! config round trip with edge validation, the preview's diff + scan,
//! the security gate refusing an execute, the §46 review mechanism
//! clearing it, the §74 job actually packaging + uploading through both
//! built-in providers, the §42 record committing only after the provider
//! answers, and the audit trail.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use serde_json::{json, Value};
use zamin_protocol::methods;

use common::{connect_daemon, make_server_root, register_server, scoped_dir, spawn_daemon};

const LONG: Duration = Duration::from_secs(30);

/// The Discord bot token shape: three base64url dot-separated segments.
/// Fake, but shaped exactly like the real thing.
const FAKE_TOKEN: &str =
    "MTE0MTQxNDE0MTQxNDE0MTQxNA.GhUiWh.SFLQuN8SxjX0COoNUbMQhPdwOOms0TbYmGo5Qu4";

fn seed_tree(root: &std::path::Path, with_secret: bool) {
    std::fs::create_dir_all(root.join("plugins/TAB")).unwrap();
    std::fs::create_dir_all(root.join("plugins/example")).unwrap();
    std::fs::write(root.join("server.properties"), "motd=Hello\n").unwrap();
    std::fs::write(
        root.join("plugins/TAB/config.yml"),
        "tablist:\n  enabled: true\n",
    )
    .unwrap();
    std::fs::write(root.join("plugins/example/config.yml"), "spam: eggs\n").unwrap();
    if with_secret {
        std::fs::create_dir_all(root.join("plugins/DiscordSRV")).unwrap();
        std::fs::write(
            root.join("plugins/DiscordSRV/config.yml"),
            format!("BotToken: \"{FAKE_TOKEN}\"\n"),
        )
        .unwrap();
    }
}

async fn publish_config_set(
    client: &mut common::Client,
    server_id: &str,
    config: Value,
) -> Result<Value, Value> {
    client
        .request(
            methods::PUBLISH_CONFIG_SET,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": server_id,
                "config": config,
            }),
        )
        .await
}

async fn preview(client: &mut common::Client, server_id: &str) -> Value {
    client
        .request(
            methods::PUBLISH_PREVIEW,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": server_id}),
        )
        .await
        .expect("preview")
}

async fn execute(
    client: &mut common::Client,
    server_id: &str,
    confirm_unsafe: bool,
) -> Result<Value, Value> {
    client
        .request(
            methods::PUBLISH_EXECUTE,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": server_id,
                "confirmUnsafe": confirm_unsafe,
            }),
        )
        .await
}

/// Poll jobs.get to a terminal state; returns (state, error).
async fn wait_job(client: &mut common::Client, job_id: &str) -> (String, Option<Value>) {
    let deadline = std::time::Instant::now() + LONG;
    loop {
        let job = client
            .request(methods::JOBS_GET, json!({"jobId": job_id}))
            .await
            .expect("jobs.get");
        let state = job["state"].as_str().unwrap_or_default().to_owned();
        if state != "running" && state != "queued" {
            return (
                state,
                job["error"].as_object().map(|_| job["error"].clone()),
            );
        }
        assert!(
            std::time::Instant::now() < deadline,
            "publish job never finished: {job:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn archive_config() -> Value {
    json!({
        "selection": {
            "includes": [
                {"kind": "folder", "path": "plugins"},
                {"kind": "file", "path": "server.properties"}
            ],
            "excludes": []
        },
        "providerId": "archive",
        "providerSettings": {},
        "title": "Box Demo",
        "description": "the demo package",
        "version": "1.0.0",
        "changelog": "first cut"
    })
}

#[tokio::test]
async fn providers_list_and_config_roundtrip_with_edge_validation() {
    let data_dir = scoped_dir("publish-config");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("publish-config");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    let root = make_server_root("publish-config-root");
    seed_tree(&root, false);
    register_server(&mut client, "test", &root).await;

    // The provider list names both honest built-ins, with the credential
    // room honestly empty for now.
    let providers = client
        .request(methods::PUBLISH_PROVIDERS_LIST, json!({}))
        .await
        .expect("providers list");
    let ids: Vec<&str> = providers["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&"archive") && ids.contains(&"local-dir"),
        "{ids:?}"
    );

    // Defaults: nothing selected, the archive provider.
    let default_config = client
        .request(
            methods::PUBLISH_CONFIG_GET,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("config get");
    assert_eq!(default_config["providerId"], "archive");
    assert!(
        default_config["selection"]["includes"]
            .as_array()
            .unwrap_or(&vec![])
            .is_empty(),
        "an untouched config selects nothing"
    );

    // Edge validation: an absolute include rule is refused before any
    // state changes.
    let mut bad = archive_config();
    bad["selection"]["includes"] = json!([{"kind": "folder", "path": "/absolute"}]);
    let err = publish_config_set(&mut client, "test", bad)
        .await
        .expect_err("absolute rule refused");
    assert_eq!(err["code"], "PROTOCOL_INVALID_REQUEST", "{err:?}");

    // Unknown provider refused.
    let mut bad = archive_config();
    bad["providerId"] = json!("builtbybit");
    let err = publish_config_set(&mut client, "test", bad)
        .await
        .expect_err("unknown provider refused");
    assert_eq!(err["code"], "PROTOCOL_INVALID_REQUEST", "{err:?}");

    // The round trip.
    let set = publish_config_set(&mut client, "test", archive_config())
        .await
        .expect("config set");
    assert_eq!(set["providerId"], "archive");
    assert_eq!(set["selection"]["includes"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn preview_diff_scan_gate_review_execute_and_record_commit() {
    let data_dir = scoped_dir("publish-e2e");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("publish-e2e");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    let root = make_server_root("publish-e2e-root");
    seed_tree(&root, true); // carries the fake DiscordSRV bot token
    register_server(&mut client, "test", &root).await;
    publish_config_set(&mut client, "test", archive_config())
        .await
        .expect("config set");

    // Preview: three selected files, all "added" against no publication,
    // with the DiscordSRV token caught (token pattern AND config key).
    let before = preview(&mut client, "test").await;
    assert_eq!(
        before["selectedFiles"], 4,
        "TAB + example + DiscordSRV + server.properties"
    );
    assert_eq!(before["counts"]["added"], 4);
    assert_eq!(before["counts"]["changed"], 4);
    let findings = before["scan"]["findings"].as_array().unwrap();
    let kinds: Vec<&str> = findings
        .iter()
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"discord-bot-token"), "{kinds:?}");
    assert!(kinds.contains(&"config-secret-key"), "{kinds:?}");
    let token_finding = findings
        .iter()
        .find(|f| f["kind"] == "discord-bot-token")
        .unwrap();
    assert_eq!(
        token_finding["severity"], "critical",
        "inside DiscordSRV escalates"
    );
    assert!(
        !token_finding["excerpt"]
            .as_str()
            .unwrap()
            .contains("MTE0MTQxNDE0"),
        "excerpts are redacted"
    );
    assert_eq!(before["blockingCount"], 2);
    assert!(before["lastPublication"].is_null(), "nothing published yet");

    // The gate: execute refuses while blocking findings exist.
    let err = execute(&mut client, "test", false)
        .await
        .expect_err("execute must refuse");
    assert_eq!(err["code"], "PUBLISH_SECRETS_DETECTED", "{err:?}");
    assert_eq!(err["context"]["blockingCount"], 2);
    assert!(err["remediation"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r == "publish-anyway"));

    // §46: review both findings (they are the same file, two kinds).
    let first = client
        .request(
            methods::PUBLISH_REVIEW_SET,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "test",
                "file": "plugins/DiscordSRV/config.yml",
                "kind": "discord-bot-token",
                "reviewed": true
            }),
        )
        .await
        .expect("review set");
    assert_eq!(
        first["blockingCount"], 1,
        "the reviewed kind stops blocking"
    );
    let second = client
        .request(
            methods::PUBLISH_REVIEW_SET,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "test",
                "file": "plugins/DiscordSRV/config.yml",
                "kind": "config-secret-key",
                "reviewed": true
            }),
        )
        .await
        .expect("review set");
    assert_eq!(second["blockingCount"], 0, "all reviewed, none blocking");
    let after_review = preview(&mut client, "test").await;
    assert_eq!(after_review["blockingCount"], 0);
    let reviewed_flags: Vec<bool> = after_review["scan"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["reviewed"].as_bool().unwrap())
        .collect();
    assert!(
        reviewed_flags.iter().all(|r| *r),
        "reviewed findings stay visible"
    );

    // Execute: the job walks §74's stages and commits the record.
    let job = execute(&mut client, "test", false).await.expect("execute");
    let job_id = job["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, error) = wait_job(&mut client, &job_id).await;
    assert_eq!(state, "succeeded", "error: {error:?}");

    // The record: last publication + receipt + package on disk.
    let state = client
        .request(
            methods::PUBLISH_STATE,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("publish state");
    assert_eq!(state["lastPublication"]["providerId"], "archive");
    assert_eq!(state["lastPublication"]["version"], "1.0.0");
    assert_eq!(state["lastPublication"]["fileCount"], 4);
    assert_eq!(state["receipt"]["providerId"], "archive");
    assert_eq!(state["packagePresent"], true);

    // The §42 diff resets: everything unchanged now.
    let after = preview(&mut client, "test").await;
    assert_eq!(after["counts"]["unchanged"], 4);
    assert_eq!(after["counts"]["changed"], 0);

    // Touch a file: the diff says "modified" and only that.
    std::fs::write(
        root.join("plugins/TAB/config.yml"),
        "tablist:\n  enabled: false\n",
    )
    .unwrap();
    let touched = preview(&mut client, "test").await;
    assert_eq!(touched["counts"]["modified"], 1);
    assert_eq!(touched["counts"]["changed"], 1);

    // The audit carries the mutations (config.set, review.set, execute).
    let audit = std::fs::read_to_string(data_dir.join("audit.log")).unwrap();
    assert!(audit.contains("publish.config.set"));
    assert!(audit.contains("publish.review.set"));
    assert!(audit.contains("publish.execute"));
}

#[tokio::test]
async fn empty_selection_refused_and_confirm_unsafe_overrides_the_gate() {
    let data_dir = scoped_dir("publish-empty");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("publish-empty");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    let root = make_server_root("publish-empty-root");
    seed_tree(&root, true);
    register_server(&mut client, "test", &root).await;

    // §41 is structural: an empty include list means refuse, never
    // "package everything".
    let mut empty = archive_config();
    empty["selection"]["includes"] = json!([]);
    publish_config_set(&mut client, "test", empty)
        .await
        .expect("empty selection is a valid CONFIG");
    let err = execute(&mut client, "test", false)
        .await
        .expect_err("empty selection refused");
    assert_eq!(err["code"], "PUBLISH_NOTHING_SELECTED", "{err:?}");

    // The gate stays honest without reviews — but the founder's §45
    // "Publish Anyway" is the explicit override, and it works.
    let back = archive_config();
    publish_config_set(&mut client, "test", back)
        .await
        .expect("config back");
    let job = execute(&mut client, "test", true)
        .await
        .expect("publish anyway");
    let job_id = job["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, error) = wait_job(&mut client, &job_id).await;
    assert_eq!(
        state, "succeeded",
        "confirmUnsafe carries the publish: {error:?}"
    );
}

#[tokio::test]
async fn local_dir_provider_uploads_the_package_and_its_receipt() {
    let data_dir = scoped_dir("publish-localdir");
    let endpoint = zamin_ipc::Endpoint::unique_for_test("publish-localdir");
    let _daemon = spawn_daemon(&data_dir, &endpoint);
    let mut client = connect_daemon(&endpoint).await;
    let root = make_server_root("publish-localdir-root");
    seed_tree(&root, false);
    register_server(&mut client, "test", &root).await;

    // Settings validation at the edge: no outDir, no acceptance.
    let mut bad = archive_config();
    bad["providerId"] = json!("local-dir");
    let err = publish_config_set(&mut client, "test", bad)
        .await
        .expect_err("local-dir without outDir refused");
    assert_eq!(err["code"], "PROTOCOL_INVALID_REQUEST", "{err:?}");

    let out_dir = scoped_dir("publish-localdir-out");
    let mut good = archive_config();
    good["providerId"] = json!("local-dir");
    good["providerSettings"] = json!({"outDir": out_dir.to_string_lossy()});
    publish_config_set(&mut client, "test", good)
        .await
        .expect("local-dir with outDir accepted");

    let job = execute(&mut client, "test", false).await.expect("execute");
    let job_id = job["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, error) = wait_job(&mut client, &job_id).await;
    assert_eq!(state, "succeeded", "{error:?}");

    // The provider's receipt, both in the state and beside the copy.
    let state = client
        .request(
            methods::PUBLISH_STATE,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "test"}),
        )
        .await
        .expect("publish state");
    assert_eq!(state["receipt"]["providerId"], "local-dir");
    let reference = state["receipt"]["reference"].as_str().unwrap().to_owned();
    let copied = out_dir.join(&reference);
    assert!(copied.is_file(), "{reference:?} landed in the out dir");
    assert!(out_dir.join("package.receipt.json").is_file());
}
