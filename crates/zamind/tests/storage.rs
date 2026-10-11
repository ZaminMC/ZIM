//! `storage.get` / `storage.set` (ADR-0034): the user-controlled data
//! root. The redirect pointer file is injected per-daemon via
//! `--storage-redirect` so these tests never touch the machine's real
//! platform config location; the wire contract and the honesty fields
//! are what's under test.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use serde_json::json;

use common::{connect_daemon, scoped_dir, spawn_daemon_with};
use zamin_protocol::methods;

fn unique_endpoint() -> zamin_ipc::Endpoint {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    zamin_ipc::Endpoint::unique_for_test(&format!("storage-{}", N.fetch_add(1, Ordering::Relaxed)))
}

/// A daemon whose redirect pointer lives inside the scoped test tree.
/// Returns the process handle: the caller MUST bind it for the whole
/// test (e.g. `let (.., _daemon) = …`) — dropping `TestServer` kills
/// the daemon, and a helper-local handle would die before the first
/// request ever connects.
async fn daemon_with_redirect(
    tag: &str,
) -> (std::path::PathBuf, zamin_ipc::Endpoint, common::TestServer) {
    let data_dir = scoped_dir(tag);
    let redirect = data_dir.join("redirect.txt");
    let endpoint = unique_endpoint();
    let redirect_str = redirect.to_str().unwrap().to_owned();
    let daemon = spawn_daemon_with(&data_dir, &endpoint, &["--storage-redirect", &redirect_str]);
    (data_dir, endpoint, daemon)
}

#[tokio::test]
async fn storage_get_maps_where_everything_lives() {
    let (data_dir, endpoint, _daemon) = daemon_with_redirect("get-map").await;
    let mut client = connect_daemon(&endpoint).await;

    let result = client
        .request(methods::STORAGE_GET, json!({}))
        .await
        .expect("storage.get");

    assert_eq!(result["root"], data_dir.to_str().unwrap());
    // The subdirectories are the daemon's actual layout, stated, not
    // guessed: instances hold the server roots, cache and java are the
    // shared artifact stores.
    assert_eq!(
        result["instancesDir"],
        data_dir.join("instances").to_str().unwrap()
    );
    assert_eq!(result["cacheDir"], data_dir.join("cache").to_str().unwrap());
    assert_eq!(result["javaDir"], data_dir.join("java").to_str().unwrap());
    // No redirect stored, and the daemon runs on an explicit --data-dir:
    // nothing pending, nothing claimed active.
    assert_eq!(result["redirectActive"], false);
    assert_eq!(result["pendingRoot"], serde_json::Value::Null);
    // The pointer's own address is part of the answer (the UI shows
    // where the preference itself lives).
    assert_eq!(
        result["redirectPath"],
        data_dir.join("redirect.txt").to_str().unwrap()
    );
    // The platform default is named so the UI can say "you are off the
    // default" — and it is an absolute path.
    assert!(
        result["defaultRoot"].as_str().unwrap().starts_with('/')
            || result["defaultRoot"].as_str().unwrap().contains(":\\"),
        "defaultRoot is absolute: {}",
        result["defaultRoot"]
    );
}

#[tokio::test]
async fn storage_set_writes_the_pointer_and_states_the_honest_facts() {
    let (data_dir, endpoint, _daemon) = daemon_with_redirect("set-pointer").await;
    let mut client = connect_daemon(&endpoint).await;

    // A not-yet-existing directory is the first-run case: it is created
    // and probed, never refused.
    let new_root = scoped_dir("set-pointer-target");
    let target = new_root.join("D-ZIM");
    let result = client
        .request(
            methods::STORAGE_SET,
            json!({ "root": target.to_str().unwrap() }),
        )
        .await
        .expect("storage.set");

    assert_eq!(result["root"], target.to_str().unwrap());
    assert_eq!(result["restartRequired"], true);
    assert_eq!(result["currentRoot"], data_dir.to_str().unwrap());
    assert_eq!(result["serversInCurrentRoot"], 0);

    // The pointer landed on disk, atomically, one absolute path.
    let stored = std::fs::read_to_string(data_dir.join("redirect.txt")).unwrap();
    assert_eq!(stored.trim(), target.to_str().unwrap());
    assert!(target.is_dir(), "the target directory was created");

    // And the read side now tells the truth about the pending state:
    // stored but not live (this daemon still runs the old root).
    let readback = client
        .request(methods::STORAGE_GET, json!({}))
        .await
        .expect("storage.get after set");
    assert_eq!(readback["redirectActive"], false);
    assert_eq!(readback["pendingRoot"], target.to_str().unwrap());
    assert_eq!(readback["root"], data_dir.to_str().unwrap());
}

#[tokio::test]
async fn a_relative_storage_root_is_a_typed_refusal() {
    let (_data_dir, endpoint, _daemon) = daemon_with_redirect("set-relative").await;
    let mut client = connect_daemon(&endpoint).await;

    // Relative on every platform (no drive letter, no leading slash) —
    // the classic accident of pasting a display path.
    let error = client
        .request(methods::STORAGE_SET, json!({ "root": "zim-data" }))
        .await
        .expect_err("a relative path must never become the data root");
    assert_eq!(error["code"], "PROTOCOL_INVALID_REQUEST", "{error}");
    // The refusal is loud and the pointer is untouched.
    let readback = client
        .request(methods::STORAGE_GET, json!({}))
        .await
        .expect("storage.get after refusal");
    assert_eq!(readback["pendingRoot"], serde_json::Value::Null);
}

#[tokio::test]
async fn a_file_where_the_root_should_be_is_refused() {
    let (data_dir, endpoint, _daemon) = daemon_with_redirect("set-file").await;
    let mut client = connect_daemon(&endpoint).await;

    let file_path = data_dir.join("not-a-dir.txt");
    std::fs::write(&file_path, b"i am a file").unwrap();
    let error = client
        .request(
            methods::STORAGE_SET,
            json!({ "root": file_path.to_str().unwrap() }),
        )
        .await
        .expect_err("a file is not a data root");
    assert_eq!(error["code"], "FS_NOT_WRITABLE", "{error}");
}

#[tokio::test]
async fn the_pointer_survives_and_never_retcons_a_running_daemon() {
    // The production steering itself (spawn with NO arguments → the
    // redirect names the root) is zamin-core's resolve_data_dir_from
    // unit matrix plus main.rs's explicit-flag precedence; what the wire
    // owes is that the stored pointer is exactly what resolve reads —
    // one absolute path, stable bytes — and that a running daemon keeps
    // its own root no matter what the pointer says.
    let (data_dir, endpoint, _daemon) = daemon_with_redirect("steer").await;
    let mut client = connect_daemon(&endpoint).await;

    let target = scoped_dir("steer-target");
    client
        .request(
            methods::STORAGE_SET,
            json!({ "root": target.to_str().unwrap() }),
        )
        .await
        .expect("storage.set");

    let stored = std::fs::read_to_string(data_dir.join("redirect.txt")).unwrap();
    assert_eq!(stored.trim(), target.to_str().unwrap());
    assert!(
        std::path::Path::new(stored.trim()).is_absolute(),
        "the pointer is an absolute path (the resolve law)"
    );

    // A stored pointer never retcons a running daemon.
    let readback = client
        .request(methods::STORAGE_GET, json!({}))
        .await
        .expect("storage.get");
    assert_eq!(readback["root"], data_dir.to_str().unwrap());
    assert_eq!(readback["pendingRoot"], target.to_str().unwrap());
}
