//! `zamin plugins` / `zamin jobs` end to end (ADR-0012): the real binary
//! drives the real daemon against a local mock catalog — search shows the
//! target the server's layout dictates, a pinned or latest-for-loader
//! install lands byte-identical as a job, `jobs list/get` see it, and the
//! delete prompt refuses a mismatched confirmation before any connection.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::{Read as _, Write as _};
use std::sync::Arc;
use std::time::Duration;

use common::{wait_until, Harness};

/// A distinct endpoint per test: the tag is hashed with the pid, so two
/// tests sharing a tag share a socket — give each test its own.
fn unique_endpoint_tag(name: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    format!("cli-plugins-{}-{}", name, N.fetch_add(1, Ordering::Relaxed))
}

struct MockHttpResponse {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl MockHttpResponse {
    fn json(body: impl Into<Vec<u8>>) -> MockHttpResponse {
        MockHttpResponse {
            status: 200,
            reason: "OK",
            content_type: "application/json",
            body: body.into(),
        }
    }
    fn bytes(body: Vec<u8>) -> MockHttpResponse {
        MockHttpResponse {
            status: 200,
            reason: "OK",
            content_type: "application/octet-stream",
            body,
        }
    }
    fn not_found() -> MockHttpResponse {
        MockHttpResponse {
            status: 404,
            reason: "Not Found",
            content_type: "application/json",
            body: br#"{"error":"not found"}"#.to_vec(),
        }
    }
}

/// An in-process HTTP server speaking canned responses by path; the CLI
/// catalog tests never touch the real network. Dropping it stops the
/// accept loop.
struct MockHttp {
    url: String,
}

impl MockHttp {
    fn spawn(handler: impl Fn(&str) -> MockHttpResponse + Send + Sync + 'static) -> MockHttp {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("mock binds");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let handler = Arc::new(handler);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            request.extend_from_slice(&buf[..n]);
                            if request.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let response = handler(&path);
                let head = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    response.reason,
                    response.content_type,
                    response.body.len()
                );
                if stream.write_all(head.as_bytes()).is_err() {
                    continue;
                }
                let _ = stream.write_all(&response.body);
                let _ = stream.flush();
            }
        });
        MockHttp { url }
    }
}

const SEARCH_JSON: &str = r#"{
  "hits": [
    {"project_id": "AABBCC", "slug": "essentialsx", "title": "EssentialsX",
     "description": "The essential plugin suite.", "downloads": 4000000,
     "icon_url": null, "loaders": ["paper", "spigot"], "categories": ["chat"]},
    {"project_id": "DDEEFF", "slug": "luckperms", "title": "LuckPerms",
     "description": "A permissions plugin.", "downloads": 3100000,
     "icon_url": null, "loaders": ["paper", "fabric"], "categories": ["management"]}
  ],
  "total": 2
}"#;

/// Two installable versions that share one file name — the update-in-place
/// case the typed replace rule exists for — plus the fabric row (a
/// sha1-only file, not installable) for the loader-filter assertions.
fn versions_json(origin: &str, sha_v1: &str, sha_v2: &str) -> String {
    format!(
        r#"[
  {{"id": "ver9", "version_number": "2.20.0", "game_versions": ["1.21.1"],
   "loaders": ["paper"], "date_published": "2026-09-01T10:00:00Z",
   "files": [{{"url": "{origin}/files/v1/EssentialsX-2.20.0.jar", "filename": "EssentialsX-2.20.0.jar",
              "size": null, "primary": true, "hashes": {{"sha1": "aa", "sha512": "{sha_v1}"}}}}]}},
  {{"id": "ver10", "version_number": "2.21.0", "game_versions": ["1.21.1"],
   "loaders": ["paper"], "date_published": "2026-10-01T10:00:00Z",
   "files": [{{"url": "{origin}/files/v2/EssentialsX-2.20.0.jar", "filename": "EssentialsX-2.20.0.jar",
              "size": null, "primary": true, "hashes": {{"sha1": "ab", "sha512": "{sha_v2}"}}}}]}},
  {{"id": "ver7", "version_number": "1.0-fabric", "game_versions": ["1.21"],
   "loaders": ["fabric"], "date_published": "2026-01-01T10:00:00Z",
   "files": [{{"url": "{origin}/files/fabric.jar", "filename": "fabric.jar",
              "size": null, "primary": true, "hashes": {{"sha1": "ff"}}}}]}}
]"#
    )
}

fn spawn_modrinth(jar_v1: Vec<u8>, jar_v2: Vec<u8>) -> MockHttp {
    // The origin URL is only known after the mock binds, so the handler
    // reads it from this cell; the test fills it before any request.
    let origin: Arc<std::sync::Mutex<String>> = Arc::new(std::sync::Mutex::new(String::new()));
    let origin_for_handler = Arc::clone(&origin);
    let sha_v1 = sha512_hex(&jar_v1);
    let sha_v2 = sha512_hex(&jar_v2);
    let mock = MockHttp::spawn(move |path| {
        if path.starts_with("/v2/search") {
            return MockHttpResponse::json(SEARCH_JSON);
        }
        if path.starts_with("/v2/project/AABBCC/version") {
            let base = origin_for_handler.lock().unwrap().clone();
            return MockHttpResponse::json(versions_json(&base, &sha_v1, &sha_v2));
        }
        if path.starts_with("/files/v1/") {
            return MockHttpResponse::bytes(jar_v1.clone());
        }
        if path.starts_with("/files/v2/") {
            return MockHttpResponse::bytes(jar_v2.clone());
        }
        if path.starts_with("/files/") {
            return MockHttpResponse::bytes(b"fabric jar".to_vec());
        }
        MockHttpResponse::not_found()
    });
    *origin.lock().unwrap() = mock.url.clone();
    mock
}

fn sha512_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha512::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn cli_drives_the_plugin_catalog_end_to_end() {
    let jar = b"essentialsx plugin jar payload - sha512 pinned".to_vec();
    let mock = spawn_modrinth(jar.clone(), jar.clone());

    let harness = Harness::spawn_with(
        &unique_endpoint_tag("catalog"),
        &["--modrinth-url".to_owned(), mock.url.clone()],
    );
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);

    // Search: the human table names the target and the hits.
    let table = harness.zamin(&["plugins", "search", "demo", "essentials"]);
    assert!(table.status.success());
    let stdout = String::from_utf8_lossy(&table.stdout);
    assert!(
        stdout.contains("`plugins` directory"),
        "target shown: {stdout}"
    );
    assert!(stdout.contains("essentialsx"), "hits shown: {stdout}");

    // JSON mode: the raw protocol result.
    let search = harness.zamin_json(&["plugins", "search", "demo", "essentials"]);
    assert_eq!(search["target"], "plugins");
    assert_eq!(search["hits"][0]["projectId"], "AABBCC");

    // Versions: the pin list, loader-filtered by the daemon — a bukkit
    // root never sees the fabric row (a sha1-only file, not installable).
    let versions = harness.zamin_json(&["plugins", "versions", "demo", "AABBCC"]);
    let list = versions["versions"].as_array().expect("versions array");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["versionNumber"], "2.20.0");
    assert_eq!(list[1]["versionNumber"], "2.21.0");

    // Install, waiting on the job: the jar lands byte-identical.
    let installed = harness.zamin_json(&["plugins", "install", "demo", "AABBCC", "--wait"]);
    assert_eq!(installed["state"], "succeeded", "install job: {installed}");
    let landed = harness.root.join("plugins").join("EssentialsX-2.20.0.jar");
    assert!(
        wait_until(Duration::from_secs(5), || landed.exists()),
        "jar lands in the target directory"
    );
    assert_eq!(std::fs::read(&landed).unwrap(), jar, "byte-identical");

    // The inventory is the directory, read back over the wire.
    let inventory = harness.zamin_json(&["plugins", "installed", "demo"]);
    assert_eq!(inventory["target"], "plugins");
    assert_eq!(
        inventory["entries"][0]["fileName"],
        "EssentialsX-2.20.0.jar"
    );

    // jobs list and get see the install job.
    let jobs = harness.zamin_json(&["jobs", "list"]);
    let jobs = jobs["jobs"].as_array().expect("jobs array");
    let install_job = jobs
        .iter()
        .find(|job| job["kind"] == "plugins.install")
        .expect("install job listed");
    assert_eq!(install_job["state"], "succeeded");
    assert_eq!(install_job["serverId"], "demo");
    let job_id = install_job["jobId"].as_str().unwrap().to_owned();
    let job = harness.zamin_json(&["jobs", "get", &job_id]);
    assert_eq!(job["state"], "succeeded");

    // Delete --yes removes the jar; the inventory empties.
    let deleted = harness.zamin_json(&[
        "plugins",
        "delete",
        "demo",
        "EssentialsX-2.20.0.jar",
        "--yes",
    ]);
    assert_eq!(deleted["deleted"], "EssentialsX-2.20.0.jar");
    assert!(!landed.exists(), "the jar is really gone");
    let after = harness.zamin_json(&["plugins", "installed", "demo"]);
    assert_eq!(after["entries"].as_array().unwrap().len(), 0);
}

#[test]
fn cli_delete_prompt_refuses_a_mismatched_confirmation() {
    let jar = b"prompt test jar".to_vec();
    let mock = spawn_modrinth(jar.clone(), jar);

    let harness = Harness::spawn_with(
        &unique_endpoint_tag("prompt"),
        &["--modrinth-url".to_owned(), mock.url.clone()],
    );
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);

    // A jar the directory already holds — the inventory is the disk.
    let plugins_dir = harness.root.join("plugins");
    std::fs::create_dir_all(&plugins_dir).expect("plugins dir");
    let target = plugins_dir.join("Extra.jar");
    std::fs::write(&target, b"extra jar bytes").expect("jar");

    // Without --yes the CLI asks; a wrong answer deletes nothing.
    let refused = harness.zamin_confirm(
        &["plugins", "delete", "demo", "Extra.jar"],
        "not-the-name\n",
    );
    assert!(
        !refused.status.success(),
        "a mismatched confirm must refuse"
    );
    assert!(target.exists(), "the jar survived the refused delete");

    // The right answer deletes (and the connection happens after the prompt).
    let confirmed =
        harness.zamin_confirm(&["plugins", "delete", "demo", "Extra.jar"], "Extra.jar\n");
    assert!(confirmed.status.success(), "the matching confirm deletes");
    assert!(!target.exists(), "the jar is really gone this time");
}

#[test]
fn cli_updates_a_plugin_through_the_typed_replace_rule() {
    let jar_v1 = b"plugin bytes version nine".to_vec();
    let jar_v2 = b"plugin bytes version ten - the update".to_vec();
    let mock = spawn_modrinth(jar_v1.clone(), jar_v2.clone());

    let harness = Harness::spawn_with(
        &unique_endpoint_tag("update"),
        &["--modrinth-url".to_owned(), mock.url.clone()],
    );
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);
    let landed = harness.root.join("plugins").join("EssentialsX-2.20.0.jar");

    // Install the pinned 2.20.0; the jar lands byte-identical.
    let installed = harness.zamin_json(&[
        "plugins",
        "install",
        "demo",
        "AABBCC",
        "--version",
        "ver9",
        "--wait",
    ]);
    assert_eq!(installed["state"], "succeeded", "install job: {installed}");
    assert_eq!(std::fs::read(&landed).unwrap(), jar_v1);

    // Re-installing the identical file is an idempotent success: the
    // daemon short-circuits on the published sha512, no second landing.
    let again = harness.zamin_json(&[
        "plugins",
        "install",
        "demo",
        "AABBCC",
        "--version",
        "ver9",
        "--wait",
    ]);
    assert_eq!(again["state"], "succeeded", "reinstall job: {again}");
    assert_eq!(std::fs::read(&landed).unwrap(), jar_v1);

    // 2.21.0 publishes different content under the same file name: the
    // typed refusal arrives before any job exists — nothing was touched.
    let refused = harness.zamin(&[
        "--json",
        "plugins",
        "install",
        "demo",
        "AABBCC",
        "--version",
        "ver10",
        "--wait",
    ]);
    assert!(!refused.status.success(), "the update must refuse");
    let error: serde_json::Value =
        serde_json::from_slice(&refused.stdout).expect("typed error object on stdout");
    assert_eq!(error["code"], "PLUGIN_EXISTS", "error: {error}");
    assert_eq!(error["context"]["file"], "EssentialsX-2.20.0.jar");
    assert_eq!(std::fs::read(&landed).unwrap(), jar_v1, "old bytes intact");

    // --replace is the operator's decision; the new bytes land atomically.
    let replaced = harness.zamin_json(&[
        "plugins",
        "install",
        "demo",
        "AABBCC",
        "--version",
        "ver10",
        "--wait",
        "--replace",
    ]);
    assert_eq!(replaced["state"], "succeeded", "replace job: {replaced}");
    assert_eq!(std::fs::read(&landed).unwrap(), jar_v2, "updated bytes");

    // Still exactly one jar: an update, not an accumulation.
    let inventory = harness.zamin_json(&["plugins", "installed", "demo"]);
    assert_eq!(inventory["entries"].as_array().unwrap().len(), 1);
}
