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

fn sha512_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha512};
    let mut h = Sha512::new();
    h.update(bytes);
    h.finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

/// The catalog for the updates test: `version_file/{hash}` answers what
/// the catalog knows per digest (404 for never-published bytes), the
/// project's version list publishes ver9 — whose primary file is the
/// jar bytes the /files/ route serves, sha-pinned to `latest_sha`.
fn spawn_updates_modrinth(latest_jar: Vec<u8>, latest_sha: String, stale_sha: String) -> MockHttp {
    let jar_for_files = latest_jar.clone();
    let latest_for_versions = latest_sha.clone();
    let origin: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let origin_for_handler = Arc::clone(&origin);
    let latest_for_handler = latest_sha.clone();
    let stale_for_handler = stale_sha.clone();
    let mock = MockHttp::spawn(move |path| {
        if let Some(rest) = path.strip_prefix("/v2/version_file/") {
            let hash = rest.split('?').next().unwrap_or("");
            if hash == latest_for_handler {
                return MockHttpResponse::json(format!(
                    r#"{{"id": "ver9", "project_id": "AABBCC", "version_number": "2.20.0",
                        "game_versions": ["1.21.1"], "loaders": ["paper"],
                        "date_published": "2026-09-01T10:00:00Z",
                        "files": [{{"url": "http://mock/files/EssentialsX-2.20.0.jar",
                                   "filename": "EssentialsX-2.20.0.jar", "size": null,
                                   "primary": true,
                                   "hashes": {{"sha1": "aa", "sha512": "{latest_for_handler}"}}}}]}}"#
                ));
            }
            if hash == stale_for_handler {
                return MockHttpResponse::json(format!(
                    r#"{{"id": "ver8", "project_id": "AABBCC", "version_number": "2.19.0",
                        "game_versions": ["1.21.1"], "loaders": ["paper"],
                        "date_published": "2026-03-01T10:00:00Z",
                        "files": [{{"url": "http://mock/files/EssentialsX-2.19.0.jar",
                                   "filename": "EssentialsX-2.19.0.jar", "size": null,
                                   "primary": true,
                                   "hashes": {{"sha1": "ee", "sha512": "{stale_for_handler}"}}}}]}}"#
                ));
            }
            return MockHttpResponse::not_found();
        }
        if path.starts_with("/v2/project/AABBCC/version") {
            let base = origin_for_handler.lock().unwrap().clone();
            return MockHttpResponse::json(versions_json(&base, &latest_for_versions));
        }
        if path.starts_with("/files/") {
            return MockHttpResponse::bytes(jar_for_files.clone());
        }
        MockHttpResponse::not_found()
    });
    *origin.lock().unwrap() = mock.url.clone();
    mock
}

#[tokio::test]
async fn plugins_updates_reports_verdicts_and_the_install_recipe_applies() {
    // Three jars, three verdicts: the published newest bytes (up to
    // date), the previous release's bytes (update available), and bytes
    // the catalog never carried (unmanaged).
    let latest_jar: Vec<u8> = (0..128 * 1024u32).map(|i| (i % 251) as u8).collect();
    let latest_sha = sha512_hex(&latest_jar);
    let stale_jar: Vec<u8> = (0..96 * 1024u32).map(|i| (i % 233) as u8).collect();
    let stale_sha = sha512_hex(&stale_jar);
    let manual_jar = b"dropped in by hand, published nowhere".to_vec();

    let modrinth = spawn_updates_modrinth(latest_jar.clone(), latest_sha, stale_sha);

    let data_dir = scoped_dir("plugins-updates");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--modrinth-url", &modrinth.url]);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("updates-target");
    register_server(&mut client, "plug", &root).await;

    // No plugins directory yet: an empty report, not an error.
    let result = client
        .request(methods::PLUGINS_UPDATES, json!({"serverId": "plug"}))
        .await
        .expect("plugins.updates on an empty server");
    assert_eq!(result["target"], "plugins");
    assert_eq!(result["entries"].as_array().unwrap().len(), 0);

    // Seed the inventory the honest way: a catalog install (the newest
    // bytes) and a stale jar dropped on disk by the test.
    let plugins_dir = root.join("plugins");
    let result = client
        .request(
            methods::PLUGINS_INSTALL,
            json!({"serverId": "plug", "projectId": "AABBCC", "versionId": "ver9"}),
        )
        .await
        .expect("install the newest jar");
    let job_id = result["job"]["jobId"].as_str().expect("jobId").to_owned();
    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded", "error: {error:?}");
    std::fs::write(plugins_dir.join("EssentialsX-2.19.0.jar"), &stale_jar).unwrap();
    std::fs::write(plugins_dir.join("hand-dropped.jar"), &manual_jar).unwrap();

    // The check: sorted by name, every verdict honest, and the stale
    // entry carries the full recipe — project id, both version numbers,
    // and the pin that applies the update.
    let result = client
        .request(methods::PLUGINS_UPDATES, json!({"serverId": "plug"}))
        .await
        .expect("plugins.updates");
    let entries = result["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["fileName"], "EssentialsX-2.19.0.jar");
    assert_eq!(entries[0]["status"], "update-available");
    assert_eq!(entries[0]["projectId"], "AABBCC");
    assert_eq!(entries[0]["installedVersion"], "2.19.0");
    assert_eq!(entries[0]["latestVersion"], "2.20.0");
    assert_eq!(entries[0]["latestVersionId"], "ver9");

    assert_eq!(entries[1]["fileName"], "EssentialsX-2.20.0.jar");
    assert_eq!(entries[1]["status"], "up-to-date");

    assert_eq!(entries[2]["fileName"], "hand-dropped.jar");
    assert_eq!(entries[2]["status"], "unmanaged");
    assert!(entries[2].get("projectId").is_none());

    // The recipe applies through the exact flow the panel uses —
    // install with the pin and replace, per the update rule.
    let result = client
        .request(
            methods::PLUGINS_INSTALL,
            json!({
                "serverId": "plug", "projectId": "AABBCC",
                "versionId": "ver9", "replace": true
            }),
        )
        .await
        .expect("apply the update");
    let job_id = result["job"]["jobId"].as_str().expect("jobId").to_owned();
    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded", "error: {error:?}");
    // The update replaced the wrong-version file's neighbor: the newest
    // jar is in place byte-identical, the stale jar still its own file.
    assert_eq!(
        std::fs::read(plugins_dir.join("EssentialsX-2.20.0.jar")).unwrap(),
        latest_jar
    );
    assert_eq!(
        std::fs::read(plugins_dir.join("EssentialsX-2.19.0.jar")).unwrap(),
        stale_jar
    );

    // An unknown server is the usual typed miss.
    let error = client
        .request(methods::PLUGINS_UPDATES, json!({"serverId": "ghost"}))
        .await
        .expect_err("ghost server");
    assert_eq!(error["code"], "SERVER_NOT_FOUND");
}

#[tokio::test]
async fn an_update_that_retires_leaves_exactly_one_jar() {
    // The live smoke's finding (real ViaVersion jars, real Modrinth):
    // the overwrite rule is name-keyed and a version bump changes the
    // published name, so an update that only installs leaves BOTH jars
    // on disk — two versions of one plugin, which a real server refuses
    // to load. The recipe's apply therefore carries `retireFile`: the
    // row the update verdict came from, removed after the new bytes
    // land and verify.
    let latest_jar: Vec<u8> = (0..64 * 1024u32).map(|i| (i % 241) as u8).collect();
    let latest_sha = sha512_hex(&latest_jar);
    let stale_jar: Vec<u8> = (0..48 * 1024u32).map(|i| (i % 229) as u8).collect();
    let stale_sha = sha512_hex(&stale_jar);

    let modrinth = spawn_updates_modrinth(latest_jar.clone(), latest_sha, stale_sha);

    let data_dir = scoped_dir("plugins-retire");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--modrinth-url", &modrinth.url]);
    let mut client = connect_daemon(&endpoint).await;

    let root = make_server_root("retire-target");
    register_server(&mut client, "plug", &root).await;
    let plugins_dir = root.join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    std::fs::write(plugins_dir.join("EssentialsX-2.19.0.jar"), &stale_jar).unwrap();

    // Apply the recipe the panel sends: pin + replace + retire. One job,
    // one jar: the new version lands, the old file goes.
    let result = client
        .request(
            methods::PLUGINS_INSTALL,
            json!({
                "serverId": "plug", "projectId": "AABBCC",
                "versionId": "ver9", "replace": true,
                "retireFile": "EssentialsX-2.19.0.jar"
            }),
        )
        .await
        .expect("apply the update with the retire");
    let job_id = result["job"]["jobId"].as_str().expect("jobId").to_owned();
    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded", "error: {error:?}");
    assert_eq!(
        std::fs::read(plugins_dir.join("EssentialsX-2.20.0.jar")).unwrap(),
        latest_jar
    );
    assert!(!plugins_dir.join("EssentialsX-2.19.0.jar").exists());

    // Retiring the name the install LANDED collapses to a no-op before
    // any job exists — the guard, not a delete of the new bytes.
    let result = client
        .request(
            methods::PLUGINS_INSTALL,
            json!({
                "serverId": "plug", "projectId": "AABBCC",
                "versionId": "ver9", "replace": true,
                "retireFile": "EssentialsX-2.20.0.jar"
            }),
        )
        .await
        .expect("same-name retire is a no-op");
    let job_id = result["job"]["jobId"].as_str().expect("jobId").to_owned();
    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded", "error: {error:?}");
    assert!(plugins_dir.join("EssentialsX-2.20.0.jar").exists());

    // An already-absent file is a successful no-op: the operator may
    // have removed the old jar while the job ran.
    let result = client
        .request(
            methods::PLUGINS_INSTALL,
            json!({
                "serverId": "plug", "projectId": "AABBCC",
                "versionId": "ver9", "replace": true,
                "retireFile": "Already-Removed.jar"
            }),
        )
        .await
        .expect("absent retire is fine");
    let job_id = result["job"]["jobId"].as_str().expect("jobId").to_owned();
    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded", "error: {error:?}");
    assert!(plugins_dir.join("EssentialsX-2.20.0.jar").exists());

    // An unsafe retire name is a typed refusal at request time — before
    // any job exists, the java.install convention.
    let error = client
        .request(
            methods::PLUGINS_INSTALL,
            json!({
                "serverId": "plug", "projectId": "AABBCC",
                "versionId": "ver9", "replace": true,
                "retireFile": "../escape.jar"
            }),
        )
        .await
        .expect_err("unsafe retire name");
    assert_eq!(error["code"], "ARCHIVE_UNSAFE_ENTRY");

    // A symlink that leaves the server root is refused the same way,
    // also before any job exists (the rooted fs is the choke point).
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc", plugins_dir.join("escape-link.jar")).unwrap();
        let error = client
            .request(
                methods::PLUGINS_INSTALL,
                json!({
                    "serverId": "plug", "projectId": "AABBCC",
                    "versionId": "ver9", "replace": true,
                    "retireFile": "escape-link.jar"
                }),
            )
            .await
            .expect_err("escaping symlink retire");
        assert_eq!(error["code"], "FS_PATH_ESCAPES_ROOT");
        std::fs::remove_file(plugins_dir.join("escape-link.jar")).unwrap();
        assert!(plugins_dir.join("EssentialsX-2.20.0.jar").exists());
    }
}
