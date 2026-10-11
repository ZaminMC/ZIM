//! The registry self-heal e2e (the founder's "always disconnected" root):
//! a registry.json torn by a hard kill used to make every daemon boot
//! `exit(1)` — the panel spawned corpse after corpse, the operator read
//! an eternal "Connecting to ZIM…", and stderr was nulled so no word of
//! why ever reached a human. The law now: quarantine the unreadable
//! file, rebuild from the `.zamin/server.json` instance markers (the
//! anchors that outrank the registry), and START — a torn registry must
//! cost the operator nothing.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{connect_daemon, endpoint_for, scoped_dir, spawn_daemon};
use serde_json::json;

/// A managed-looking instance dir: the marker binds it to its id.
fn marker_instance(data_dir: &std::path::Path, id: &str) -> std::path::PathBuf {
    let root = data_dir.join("instances").join(id);
    std::fs::create_dir_all(&root).unwrap();
    zamin_core::server::marker::write_marker(
        &root,
        &zamin_core::server::ServerId::parse(id).unwrap(),
    )
    .unwrap();
    std::fs::write(root.join("eula.txt"), "eula=true\n").unwrap();
    root
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torn_registry_quarantines_and_self_heals_from_the_markers() {
    let data_dir = scoped_dir("heal-data");
    let inner = data_dir.join("data");
    std::fs::create_dir_all(&inner).unwrap();

    // A server the registry used to know: its marker survives on disk.
    marker_instance(&inner, "survival");

    // The torn registry — the exact wound a taskkill mid-write leaves.
    std::fs::write(inner.join("registry.json"), b"{\"schemaVersion\":1,\"serv").unwrap();

    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon(&inner, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    // The daemon is UP (the old law exited(1) here) and the fleet is back.
    let list = client
        .request("server.list", json!({}))
        .await
        .expect("daemon answers with the healed registry");
    let servers = list["servers"].as_array().expect("servers array");
    assert!(
        servers.iter().any(|s| s["serverId"] == json!("survival")),
        "the marker-carrying instance must re-register itself; got {servers:?}"
    );

    // The evidence is preserved, the torn file is gone from the way.
    let corrupt: Vec<_> = std::fs::read_dir(&inner)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("registry.json.corrupt-")
        })
        .collect();
    assert_eq!(corrupt.len(), 1, "exactly one quarantined copy");
    assert!(corrupt[0].path().is_file());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_registryless_data_dir_boots_empty_never_exits() {
    let data_dir = scoped_dir("heal-empty");
    let inner = data_dir.join("data");
    std::fs::create_dir_all(&inner).unwrap();

    // No registry.json, no instances — the first boot of a fresh install.
    // The daemon must come up and answer an empty fleet.
    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon(&inner, &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    let list = client
        .request("server.list", json!({}))
        .await
        .expect("daemon answers on a fresh data dir");
    assert!(
        list["servers"]
            .as_array()
            .expect("servers array")
            .is_empty(),
        "a fresh install has no servers"
    );
}
