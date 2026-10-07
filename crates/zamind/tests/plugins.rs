//! `plugins.*` (ADR-0012): the daemon speaks Modrinth on the operator's
//! behalf — loader-faceted search, version pinning, a sha512-verified
//! install job into the directory the server's layout dictates, and a
//! delete that sanitizes the wire's filename. The catalog is a local
//! mock speaking the observed API shapes; the network is never touched.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use common::{
    connect_daemon, make_server_root, register_server, scoped_dir, spawn_daemon_with, MockHttp,
    MockHttpResponse,
};
use zamin_protocol::methods;

/// A distinct endpoint per test: the tag is hashed with the pid, so two
/// tests sharing a tag share a socket — give each test its own.
fn unique_endpoint() -> zamin_ipc::Endpoint {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    zamin_ipc::Endpoint::unique_for_test(&format!("plugins-{}", N.fetch_add(1, Ordering::Relaxed)))
}

const SEARCH_JSON: &str = r#"{
  "hits": [
    {"project_id": "AABBCC", "slug": "essentialsx", "title": "EssentialsX",
     "description": "The essential plugin suite.", "downloads": 4000000,
     "icon_url": null, "loaders": ["paper", "spigot"], "categories": ["chat"]}
  ],
  "total": 1
}"#;

/// The published sha512 rides in as the test's real digest; the fabric
/// row carries a sha1-only file on purpose (not installable).
fn versions_json(origin: &str, sha512: &str) -> String {
    format!(
        r#"[
  {{"id": "ver9", "version_number": "2.20.0", "game_versions": ["1.21.1"],
   "loaders": ["paper"], "date_published": "2026-09-01T10:00:00Z",
   "files": [{{"url": "{origin}/files/EssentialsX-2.20.0.jar", "filename": "EssentialsX-2.20.0.jar",
              "size": null, "primary": true, "hashes": {{"sha1": "aa", "sha512": "{sha512}"}}}}]}},
  {{"id": "ver7", "version_number": "1.0-fabric", "game_versions": ["1.21"],
   "loaders": ["fabric"], "date_published": "2026-01-01T10:00:00Z",
   "files": [{{"url": "{origin}/files/fabric.jar", "filename": "fabric.jar",
              "size": null, "primary": true, "hashes": {{"sha1": "ff"}}}}]}}
]"#
    )
}

fn spawn_modrinth(jar: Vec<u8>, sha512: String) -> (MockHttp, Arc<Mutex<String>>) {
    // The origin URL is only known after the mock binds, so the handler
    // reads it from this cell; the test fills it before any request.
    let origin: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let origin_for_handler = Arc::clone(&origin);
    let jar_for_handler = jar.clone();
    let sha_for_versions = sha512.clone();
    let mock = MockHttp::spawn(move |path| {
        if path.starts_with("/v2/search") {
            return MockHttpResponse::json(SEARCH_JSON);
        }
        if path.starts_with("/v2/project/AABBCC/version") {
            let base = origin_for_handler.lock().unwrap().clone();
            return MockHttpResponse::json(versions_json(&base, &sha_for_versions));
        }
        if path.starts_with("/files/") {
            return MockHttpResponse::bytes(jar_for_handler.clone());
        }
        MockHttpResponse::not_found()
    });
    *origin.lock().unwrap() = mock.url.clone();
    (mock, origin)
}

#[tokio::test]
async fn plugins_search_install_installed_delete_over_the_wire() {
    let jar: Vec<u8> = (0..256 * 1024u32).map(|i| (i % 241) as u8).collect();
    let sha = {
        use sha2::{Digest, Sha512};
        let mut h = Sha512::new();
        h.update(&jar);
        h.finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let (modrinth, _origin) = spawn_modrinth(jar.clone(), sha);

    let data_dir = scoped_dir("plugins-e2e");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--modrinth-url", &modrinth.url]);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("plugins-target");
    register_server(&mut client, "plug", &root).await;

    // --- search: the target follows the root's layout (plugins/) ------
    let result = client
        .request(
            methods::PLUGINS_SEARCH,
            json!({"serverId": "plug", "query": "essentials"}),
        )
        .await
        .expect("plugins.search");
    assert_eq!(result["target"], "plugins");
    assert_eq!(result["hits"][0]["projectId"], "AABBCC");
    assert_eq!(result["hits"][0]["title"], "EssentialsX");

    // --- versions: loader-filtered (the fabric row is gone) -----------
    let result = client
        .request(
            methods::PLUGINS_VERSIONS,
            json!({"serverId": "plug", "projectId": "AABBCC"}),
        )
        .await
        .expect("plugins.versions");
    let versions = result["versions"].as_array().expect("versions array");
    assert_eq!(versions.len(), 1, "only the paper version survives");
    assert_eq!(versions[0]["id"], "ver9");
    assert_eq!(versions[0]["fileName"], "EssentialsX-2.20.0.jar");

    // --- install: a job, and the jar lands verified --------------------
    let result = client
        .request(
            methods::PLUGINS_INSTALL,
            json!({"serverId": "plug", "projectId": "AABBCC", "versionId": "ver9"}),
        )
        .await
        .expect("plugins.install");
    assert_eq!(result["kind"], "plugins.install");
    let job_id = result["job"]["jobId"].as_str().expect("jobId").to_owned();
    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded", "error: {error:?}");

    // The target directory was created on first install; the file is
    // byte-identical and the sha512 was the published one.
    let installed_path = root.join("plugins").join("EssentialsX-2.20.0.jar");
    assert_eq!(std::fs::read(&installed_path).unwrap(), jar);

    // --- installed: the directory is the inventory ---------------------
    let result = client
        .request(methods::PLUGINS_INSTALLED, json!({"serverId": "plug"}))
        .await
        .expect("plugins.installed");
    assert_eq!(result["target"], "plugins");
    let entries = result["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["fileName"], "EssentialsX-2.20.0.jar");
    assert_eq!(entries[0]["sizeBytes"], jar.len() as u64);
    assert_eq!(entries[0]["symlinkOutside"], false);

    // --- delete: the wire's name is sanitized before the disk ----------
    let other = root.join("plugins").join("other.jar");
    std::fs::write(&other, b"another plugin").unwrap();
    client
        .request(
            methods::PLUGINS_DELETE,
            json!({"serverId": "plug", "fileName": "other.jar"}),
        )
        .await
        .expect("plugins.delete");
    assert!(!other.exists(), "the jar is gone");

    let error = client
        .request(
            methods::PLUGINS_DELETE,
            json!({"serverId": "plug", "fileName": "../EssentialsX-2.20.0.jar"}),
        )
        .await
        .expect_err("a traversal name is a typed refusal");
    assert_eq!(error["code"], "ARCHIVE_UNSAFE_ENTRY");
    assert!(
        installed_path.exists(),
        "the traversal did not delete the real jar"
    );

    // --- an unknown server is a typed refusal ---------------------------
    let error = client
        .request(methods::PLUGINS_INSTALLED, json!({"serverId": "ghost"}))
        .await
        .expect_err("ghost server");
    assert_eq!(error["code"], "SERVER_NOT_FOUND");
}

#[tokio::test]
async fn mods_target_loader_filter_and_typed_rejections() {
    let jar = b"payload".to_vec();
    let (modrinth, _origin) = spawn_modrinth(jar, "00".repeat(64));

    let data_dir = scoped_dir("plugins-mods");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--modrinth-url", &modrinth.url]);
    let mut client = connect_daemon(&endpoint).await;

    // A mods server: the root's `mods/` directory decides the target.
    let root = make_server_root("mods-target");
    std::fs::create_dir_all(root.join("mods")).unwrap();
    register_server(&mut client, "fabricbox", &root).await;

    let result = client
        .request(
            methods::PLUGINS_SEARCH,
            json!({"serverId": "fabricbox", "query": "x"}),
        )
        .await
        .expect("plugins.search");
    assert_eq!(result["target"], "mods", "the mods directory decides");

    // An unknown project id is a 404 at the catalog: CATALOG_NOT_FOUND,
    // at request time — before any job exists (the java.install rule).
    let error = client
        .request(
            methods::PLUGINS_INSTALL,
            json!({"serverId": "fabricbox", "projectId": "ZZZZZZ"}),
        )
        .await
        .expect_err("unknown project");
    assert_eq!(error["code"], "CATALOG_NOT_FOUND");

    // A project whose only loader-matching version carries a sha1-only
    // file resolves to the same typed rejection, at request time.
    let error = client
        .request(
            methods::PLUGINS_INSTALL,
            json!({"serverId": "fabricbox", "projectId": "AABBCC"}),
        )
        .await
        .expect_err("no installable version for the loader");
    assert_eq!(error["code"], "CATALOG_NOT_FOUND");
}
