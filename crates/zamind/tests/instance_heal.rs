//! The instance-deed heal e2e (the founder's "lock files"): a create job
//! killed mid-flight left an UNREGISTERED directory under `instances/`
//! behind, and every later `server.create` for that id was refused with
//! "remove it or register it instead" — the operator hand-deleted the
//! directory ("after clearing lock files, I was able to freely create
//! servers"). The laws now:
//! 1. A torn creation (no marker) is QUARANTINED beside itself and the
//!    create proceeds — evidence preserved, never destroyed;
//! 2. A VALID marker re-registers the server on the create path (and at
//!    every boot, healthy registry included) — the marker is the deed;
//! 3. `server.remove` revokes the deed, so a removed server stays
//!    removed and its id is immediately reusable.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{
    connect_daemon, endpoint_for, scoped_dir, spawn_daemon_with, MockHttp, MockHttpResponse,
};
use serde_json::{json, Value};
use zamin_protocol::methods;

fn spawn_catalog() -> MockHttp {
    use sha2::{Digest, Sha256};
    let jar: Vec<u8> = (0..64 * 1024u32).map(|i| (i % 241) as u8).collect();
    let mut hasher = Sha256::new();
    hasher.update(&jar);
    let sha: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let base: std::sync::Arc<std::sync::OnceLock<String>> =
        std::sync::Arc::new(std::sync::OnceLock::new());
    let base_for_handler = std::sync::Arc::clone(&base);
    let mock = MockHttp::spawn(move |path| match path {
        "/projects/paper" => MockHttpResponse::json(
            r#"{"project":{"id":"paper","name":"Paper"},"versions":{"1.21":["1.21.11"]}}"#,
        ),
        "/projects/paper/versions/1.21.11" => MockHttpResponse::json(
            r#"{"version":{"id":"1.21.11","java":{"version":{"minimum":21}}},"builds":[34]}"#,
        ),
        "/projects/paper/versions/1.21.11/builds" => {
            let base = base_for_handler.get().cloned().unwrap_or_default();
            MockHttpResponse::json(
                serde_json::to_vec(&json!([{
                    "id": 34,
                    "time": "2025-12-09T16:42:49Z",
                    "channel": "DEFAULT",
                    "downloads": {
                        "server:default": {
                            "name": "paper-1.21.11-34.jar",
                            "checksums": {"sha256": sha},
                            "size": jar.len(),
                            "url": format!("{base}/files/paper.jar"),
                        }
                    }
                }]))
                .unwrap(),
            )
        }
        "/files/paper.jar" => MockHttpResponse::bytes(jar.clone()),
        _ => MockHttpResponse::not_found(),
    });
    let _ = base.set(mock.url.clone());
    mock
}

fn create_params(server_id: &str) -> Value {
    json!({
        "requestId": uuid::Uuid::now_v7().to_string(),
        "serverId": server_id,
        "displayName": "Healed Server",
        "project": "paper",
        "version": "1.21.11",
        "build": 34,
    })
}

/// The exact wound a killed create job leaves: an instances/<id> tree
/// with template files but NO marker (the marker is written at the
/// registering step, the job's last beat).
fn torn_instance(data_dir: &std::path::Path, id: &str) -> std::path::PathBuf {
    let root = data_dir.join("instances").join(id);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("eula.txt"), "eula=false\n").unwrap();
    std::fs::write(root.join("half-written.jar"), b"torn download").unwrap();
    root
}

/// The old lock: a real server whose registry row was lost (registry
/// quarantine while the dir was away, hand-copied tree) — but its
/// ownership marker survives.
fn marked_instance(data_dir: &std::path::Path, id: &str) -> std::path::PathBuf {
    let root = torn_instance(data_dir, id);
    std::fs::write(root.join("eula.txt"), "eula=true\n").unwrap();
    std::fs::remove_file(root.join("half-written.jar")).unwrap();
    std::fs::write(root.join("server.jar"), b"real jar").unwrap();
    zamin_core::server::marker::write_marker(
        &root,
        &zamin_core::server::ServerId::parse(id).unwrap(),
    )
    .unwrap();
    root
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torn_creation_is_quarantined_and_the_create_proceeds() {
    let catalog = spawn_catalog();
    let data_dir = scoped_dir("heal-torn");
    let inner = data_dir.join("data");
    std::fs::create_dir_all(&inner).unwrap();

    let torn = torn_instance(&inner, "survival");
    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon_with(&inner, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    // The create that used to be refused forever ("remove it or register
    // it instead") now SUCCEEDS.
    let result = client
        .request(methods::SERVER_CREATE, create_params("survival"))
        .await
        .expect("the torn directory must not block creation");
    let job_id = result["job"]["jobId"].as_str().expect("jobId").to_owned();
    let (_state, error) =
        MockHttp::wait_job(&mut client, &job_id, std::time::Duration::from_secs(60)).await;
    assert!(
        error.is_none(),
        "the creation job itself must succeed: {error:?}"
    );

    // The fleet holds the NEW server; the OLD tree was set aside whole.
    let list = client
        .request(methods::SERVER_LIST, json!({}))
        .await
        .expect("list");
    let entry = list["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["serverId"] == json!("survival"))
        .cloned()
        .expect("the created server is registered");
    assert_eq!(entry["displayName"], "Healed Server");

    let torn_copies: Vec<_> = std::fs::read_dir(inner.join("instances"))
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("survival.torn-")
        })
        .collect();
    assert_eq!(torn_copies.len(), 1, "exactly one quarantined copy");
    assert!(
        torn_copies[0].path().join("half-written.jar").is_file(),
        "the torn tree is preserved, not deleted"
    );
    // The old path is the NEW server now: fresh files, its own deed.
    assert!(
        torn.join("server.jar").is_file(),
        "the creation filled the freed path"
    );
    assert!(
        torn.join(".zamin/server.json").is_file(),
        "the new server carries its own ownership marker"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_marked_directory_reregisters_on_create_and_the_fleet_shows_it() {
    let catalog = spawn_catalog();
    let data_dir = scoped_dir("heal-marked");
    let inner = data_dir.join("data");
    std::fs::create_dir_all(&inner).unwrap();

    let marked = marked_instance(&inner, "survival");
    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon_with(&inner, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    // The every-boot reconcile legitimately registers a marker that was
    // on disk at boot — so this test plants a SECOND deed mid-session
    // (the hand-copied tree while ZIM runs): a new instance dir with a
    // valid marker the registry has never seen.
    let copied = marked_instance(&inner, "copied");
    let _ = copied;
    let _ = marked;

    // The create is still refused (the id is taken), but the refusal now
    // HEALS: the marker outranks the missing row, the server comes back.
    let error = client
        .request(methods::SERVER_CREATE, create_params("copied"))
        .await
        .expect_err("the id is genuinely taken");
    let message = error["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("re-registered"),
        "the refusal must say the server was re-registered: {message}"
    );

    let list = client
        .request(methods::SERVER_LIST, json!({}))
        .await
        .expect("list");
    let entry = list["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["serverId"] == json!("copied"))
        .cloned()
        .expect("the marked server was re-registered into the fleet");
    assert_eq!(entry["displayName"], json!("copied"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_marked_directory_reregisters_at_every_boot_even_with_a_healthy_registry() {
    let data_dir = scoped_dir("heal-boot");
    let inner = data_dir.join("data");
    std::fs::create_dir_all(&inner).unwrap();

    marked_instance(&inner, "survival");
    // A HEALTHY registry that simply lacks the row (the create job died
    // between the registry write and the marker write — or the marker
    // tree was copied in while the daemon was down).
    std::fs::write(
        inner.join("registry.json"),
        br#"{"schemaVersion":1,"servers":[]}"#,
    )
    .unwrap();

    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon_with(&inner, &endpoint, &[]);
    let mut client = connect_daemon(&endpoint).await;

    let list = client
        .request(methods::SERVER_LIST, json!({}))
        .await
        .expect("list");
    let found = list["servers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["serverId"] == json!("survival"));
    assert!(
        found,
        "a marked instance must register itself at boot; got {list:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_server_revokes_the_deed_and_frees_the_id() {
    let data_dir = scoped_dir("heal-remove");
    let inner = data_dir.join("data");
    std::fs::create_dir_all(&inner).unwrap();
    let marked = marked_instance(&inner, "survival");

    let endpoint = endpoint_for(&data_dir);
    let _daemon = spawn_daemon_with(&inner, &endpoint, &[]);
    let mut client = connect_daemon(&endpoint).await;

    client
        .request(
            methods::SERVER_REMOVE,
            json!({
                "requestId": uuid::Uuid::now_v7().to_string(),
                "serverId": "survival",
            }),
        )
        .await
        .expect("remove");

    // The world files stay (removal is not destruction)…
    assert!(marked.is_dir(), "the instance tree remains as plain data");
    assert!(
        marked.join("server.jar").is_file(),
        "the server's own files are untouched"
    );
    // …but the deed is gone: no boot can resurrect it, and the id is
    // free for a future register.
    assert!(
        !marked.join(".zamin/server.json").exists(),
        "the ownership marker must be revoked"
    );
}
