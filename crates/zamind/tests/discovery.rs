//! Discovery e2e (founder §64, ADR-0027): the daemon answers "what servers
//! does this machine have" from the registry and from a real scan —
//! registered servers first with their live state, unregistered server
//! directories and supported jars next, a managed directory never listed
//! twice, and the honesty fields (roots, skipped roots, budget) riding
//! every answer.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{connect_daemon, endpoint_for, register_server, scoped_dir, spawn_daemon};
use serde_json::json;
use std::path::{Path, PathBuf};

fn dir_with_properties(root: &Path, name: &str, port: u16) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("server.properties"),
        format!("motd=found by discovery\nserver-port={port}\n"),
    )
    .unwrap();
    dir
}

#[tokio::test(flavor = "multi_thread")]
async fn discover_merges_registry_and_scan_without_double_naming() {
    let data_dir = scoped_dir("data-discover");
    let fixture = scoped_dir("roots-discover");
    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon(&data_dir.join("data"), &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    // A managed server: registered, root inside a dedicated scoped parent
    // that is ALSO a configured scan root — the scan meets the managed
    // root and the merge must not name it twice (its fake server.jar
    // would otherwise resurface as a jar candidate).
    let managed_parent = scoped_dir("roots-managed-parent");
    let root = managed_parent.join("managed-root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("eula.txt"), "eula=true\n").unwrap();
    std::fs::write(root.join("server.jar"), b"fake jar bytes").unwrap();
    register_server(&mut client, "managed", &root).await;

    // An unregistered server directory, one nested deeper, and a jar.
    dir_with_properties(&fixture, "survival", 25565);
    dir_with_properties(&fixture, "group/creative", 25566);
    std::fs::create_dir_all(fixture.join("downloads")).unwrap();
    std::fs::write(fixture.join("downloads/paper-1.21.4.jar"), b"jar").unwrap();
    std::fs::write(fixture.join("downloads/library.jar"), b"jar").unwrap();

    // Configure the scan roots through the wire (and prove the round trip).
    let roots_payload = json!({
        "roots": [
            fixture.to_string_lossy(),
            managed_parent.to_string_lossy(),
        ]
    });
    let set = client
        .request(methods::DISCOVERY_ROOTS_SET, roots_payload)
        .await
        .expect("roots set");
    assert_eq!(set["roots"].as_array().unwrap().len(), 2);

    let result = client
        .request(methods::SERVER_DISCOVER, json!({}))
        .await
        .expect("discover");

    let servers = result["servers"].as_array().expect("servers array");
    let find = |id: &str| {
        servers
            .iter()
            .find(|s| {
                s["serverId"].as_str() == Some(id)
                    || s["path"].as_str().map(|p| p.ends_with(id)).unwrap_or(false)
            })
            .unwrap_or_else(|| panic!("missing {id} in {servers:?}"))
    };

    // The managed server: kind registered, its state honest, exactly once —
    // the scan walked its parent root, met the managed root and its fake
    // server.jar, and the merge dropped both duplicates.
    let managed = find("managed");
    assert_eq!(managed["kind"], "registered");
    assert_eq!(managed["state"], "not-running");
    assert_eq!(
        servers.iter().filter(|s| s["kind"] == "registered").count(),
        1,
        "a managed server is never listed twice"
    );
    assert!(
        !servers.iter().any(|s| s["kind"] != "registered"
            && s["path"]
                .as_str()
                .map(|p| p.starts_with(root.to_string_lossy().as_ref()))
                .unwrap_or(false)),
        "nothing inside a managed root is its own candidate: {servers:?}"
    );

    // The scanned candidates: directories with their ports, the jar with
    // its family evidence, the library jar absent.
    let survival = find("survival");
    assert_eq!(survival["kind"], "directory");
    assert_eq!(survival["port"], 25565);
    assert!(survival["displayName"].is_string());
    let creative = find("creative");
    assert_eq!(creative["port"], 25566);
    let paper = find("paper-1.21.4.jar");
    assert_eq!(paper["kind"], "jar");
    assert_eq!(paper["platform"], "paper");
    assert!(servers.iter().all(|s| s["path"] != "library.jar"
        && !s["path"]
            .as_str()
            .map(|p| p.ends_with("library.jar"))
            .unwrap_or(false)));

    // Honesty fields: the roots actually scanned are named (the implicit
    // instances dir only when it exists), nothing was skipped, and the
    // walk stayed in budget.
    let roots = result["roots"].as_array().unwrap();
    assert!(roots
        .iter()
        .any(|r| r.as_str() == Some(fixture.to_string_lossy().as_ref())));
    assert!(roots
        .iter()
        .any(|r| r.as_str() == Some(managed_parent.to_string_lossy().as_ref())));
    assert_eq!(result["skippedRoots"].as_array().unwrap().len(), 0);
    assert_eq!(result["truncated"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_query_filters_and_roots_refuse_relative_paths() {
    let data_dir = scoped_dir("data-discover-q");
    let fixture = scoped_dir("roots-discover-q");
    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon(&data_dir.join("data"), &endpoint);
    let mut client = connect_daemon(&endpoint).await;

    dir_with_properties(&fixture, "survival", 25565);
    dir_with_properties(&fixture, "creative", 25566);
    client
        .request(
            methods::DISCOVERY_ROOTS_SET,
            json!({ "roots": [fixture.to_string_lossy()] }),
        )
        .await
        .expect("roots set");

    let filtered = client
        .request(methods::SERVER_DISCOVER, json!({ "query": "crea" }))
        .await
        .expect("discover filtered");
    let servers = filtered["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 1, "only creative matches: {servers:?}");
    assert!(servers[0]["path"]
        .as_str()
        .map(|p| p.ends_with("creative"))
        .unwrap_or(false));

    // A relative root is a loud refusal, not a silent silent acceptance.
    let refused = client
        .request(
            methods::DISCOVERY_ROOTS_SET,
            json!({ "roots": ["relative/dir"] }),
        )
        .await
        .expect_err("relative root refused");
    assert!(refused["message"]
        .as_str()
        .map(|m| m.contains("absolute path"))
        .unwrap_or(false));

    // And a missing root is named, not faked away.
    let absent = scoped_dir("roots-absent");
    let absent_path = absent.join("no-such-root");
    client
        .request(
            methods::DISCOVERY_ROOTS_SET,
            json!({ "roots": [absent_path.to_string_lossy()] }),
        )
        .await
        .expect("absent root stored");
    let report = client
        .request(methods::SERVER_DISCOVER, json!({}))
        .await
        .expect("discover with absent root");
    let skipped = report["skippedRoots"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0]
        .as_str()
        .map(|s| s.ends_with("no-such-root"))
        .unwrap_or(false));
}

use zamin_protocol::methods;
