//! Software catalog + downloader tests. The network is never touched: a
//! tiny in-process HTTP server speaks the Fill API's observed shapes, so
//! these tests run offline on both CI platforms.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::fabric::{FabricMetaClient, MetaVersion};
use super::fill::{compare_versions, FillClient};
use super::templates::{stamp_template, template, templates, DEFAULT_TEMPLATE_ID};
use super::{download_to_dir, entry, DownloadOptions, CATALOG};
use crate::error::CoreError;
use crate::server::registry::tempdir;

// --- the mock HTTP server ------------------------------------------------

struct MockResponse {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl MockResponse {
    fn json(body: impl Into<Vec<u8>>) -> MockResponse {
        MockResponse {
            status: 200,
            reason: "OK",
            content_type: "application/json",
            body: body.into(),
        }
    }
    fn not_found() -> MockResponse {
        MockResponse {
            status: 404,
            reason: "Not Found",
            content_type: "application/json",
            body: b"{\"error\":\"not found\"}".to_vec(),
        }
    }
    fn bytes(body: Vec<u8>) -> MockResponse {
        MockResponse {
            status: 200,
            reason: "OK",
            content_type: "application/octet-stream",
            body,
        }
    }
}

struct MockServer {
    url: String,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MockServer {
    fn spawn(handler: impl Fn(&str) -> MockResponse + Send + Sync + 'static) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock binds");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        listener.set_nonblocking(true).expect("nonblocking mock");
        let handler = Arc::new(handler);
        let handle = std::thread::spawn(move || {
            // A tiny blocking poll loop; fine for tests.
            loop {
                if stop_for_thread.load(Ordering::Relaxed) {
                    return;
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nodelay(true).ok();
                        let mut request = Vec::new();
                        let mut buf = [0u8; 4096];
                        // Read until end of headers (bodies are never sent to us).
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
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
                        let _ = stream.write_all(head.as_bytes());
                        let _ = stream.write_all(&response.body);
                        let _ = stream.flush();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return,
                }
            }
        });
        MockServer {
            url,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

// --- fixtures -------------------------------------------------------------

/// The observed Fill API shapes, minor-key order scrambled on purpose.
const PROJECT_JSON: &str = r#"{
  "project": {"id": "paper", "name": "Paper"},
  "versions": {
    "1.20": ["1.20.4", "1.20.1"],
    "26.3": ["26.3", "26.3-rc-3"],
    "1.21": ["1.21.11", "1.21.11-rc3", "1.21.9", "1.21.11-pre4"]
  }
}"#;

const VERSION_JSON: &str = r#"{
  "version": {
    "id": "1.21.11",
    "java": {"version": {"minimum": 21}}
  },
  "builds": [34, 33]
}"#;

/// Version without an API java requirement — the local table decides.
const VERSION_NO_JAVA_JSON: &str = r#"{
  "version": {"id": "1.20.1"},
  "builds": [5]
}"#;

const BUILDS_JSON: &str = r#"[
  {"id": 34, "time": "2025-12-09T16:42:49Z", "channel": "ALPHA", "downloads": {
    "server:default": {"name": "paper-1.21.11-34.jar",
      "checksums": {"sha256": "ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789"},
      "size": 53333322, "url": "https://fill-data.papermc.io/v1/objects/x/paper-1.21.11-34.jar"},
    "other:thing": {"name": "ignored.jar", "checksums": {"sha256": "00"}, "url": "https://x/i.jar"}}
  },
  {"id": 33, "time": "2025-12-08T10:00:00Z", "channel": "DEFAULT", "downloads": {}},
  {"id": 32, "time": "2025-12-07T10:00:00Z", "channel": "DEFAULT", "downloads": {
    "server:default": {"name": "paper-1.21.11-32.jar",
      "checksums": {"sha256": "1111111111111111111111111111111111111111111111111111111111111111"},
      "size": 53000000, "url": "https://fill-data.papermc.io/v1/objects/y/paper-1.21.11-32.jar"}}
  }
]"#;

fn mock_fill() -> MockServer {
    MockServer::spawn(|path| match path {
        "/projects/paper" => MockResponse::json(PROJECT_JSON),
        "/projects/paper/versions/1.21.11" => MockResponse::json(VERSION_JSON),
        "/projects/paper/versions/1.20.1" => MockResponse::json(VERSION_NO_JAVA_JSON),
        "/projects/paper/versions/1.21.11/builds" => MockResponse::json(BUILDS_JSON),
        "/projects/ghost" => MockResponse::not_found(),
        _ => MockResponse::not_found(),
    })
}

fn mock_files() -> (MockServer, String, String) {
    // A 512 KiB body so downloads span several chunks and progress fires.
    let body: Vec<u8> = (0..512 * 1024).map(|i| (i % 251) as u8).collect();
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&body);
    let sha = hex(&hasher.finalize());
    let wrong_sha = "0".repeat(64);
    let body_clone = body.clone();
    let server = MockServer::spawn(move |path| match path {
        "/files/paper.jar" => MockResponse::bytes(body_clone.clone()),
        "/files/slow.jar" => {
            // Drip: sleeps between small writes exercise mid-stream cancel.
            std::thread::sleep(Duration::from_millis(50));
            MockResponse::bytes(body_clone.clone())
        }
        _ => MockResponse::not_found(),
    });
    (server, sha, wrong_sha)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// --- catalog data ----------------------------------------------------------

#[test]
fn catalog_rows_are_well_formed() {
    assert!(CATALOG.len() >= 3, "paper family rows exist");
    let ids: Vec<_> = CATALOG.iter().map(|e| e.id).collect();
    assert_eq!(
        ids.len(),
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        "unique ids"
    );
    for row in CATALOG {
        assert!(!row.name.is_empty() && !row.project.is_empty());
        assert!(entry(row.id).is_some(), "entry() finds every row");
    }
    // The families and their sources stay in sync: the Fill API family is
    // Paper/Purpur/Folia, the Fabric meta family is Fabric.
    for row in CATALOG {
        match row.id {
            "paper" | "purpur" | "folia" => {
                assert_eq!(row.source, super::SoftwareSource::Fill)
            }
            "fabric" => assert_eq!(row.source, super::SoftwareSource::FabricMeta),
            other => panic!("catalog row {other:?} is unclassified"),
        }
    }
    assert!(
        entry("forge").is_none(),
        "a family we do not speak is not in the table"
    );
}

// --- Fill client ------------------------------------------------------------

#[test]
fn versions_are_flattened_newest_first() {
    let server = mock_fill();
    let client = FillClient::new(&server.url);
    let versions = client.versions("paper").expect("versions");
    let ids: Vec<&str> = versions.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "26.3",
            "26.3-rc-3",
            "1.21.11",
            "1.21.11-rc3",
            "1.21.11-pre4",
            "1.21.9",
            "1.20.4",
            "1.20.1"
        ],
        "finals sort above their pre-releases; numerics compare numerically"
    );
}

#[test]
fn version_comparator_rules() {
    use std::cmp::Ordering;
    assert_eq!(compare_versions("1.21.11", "1.21.9"), Ordering::Greater);
    assert_eq!(
        compare_versions("1.21.11", "1.21.11-rc3"),
        Ordering::Greater
    );
    assert_eq!(compare_versions("26.3", "1.21.11"), Ordering::Greater);
    assert_eq!(compare_versions("1.20.10", "1.20.9"), Ordering::Greater);
    assert_eq!(compare_versions("1.21.11", "1.21.11"), Ordering::Equal);
}

#[test]
fn java_major_comes_from_the_api_with_local_fallback() {
    let server = mock_fill();
    let client = FillClient::new(&server.url);
    assert_eq!(
        client.version_java_major("paper", "1.21.11").expect("api"),
        Some(21),
        "the API's own requirement wins"
    );
    assert_eq!(
        client
            .version_java_major("paper", "1.20.1")
            .expect("fallback"),
        Some(17),
        "no API statement → the local table decides"
    );
}

#[test]
fn builds_are_newest_first_and_skip_incomplete() {
    let server = mock_fill();
    let client = FillClient::new(&server.url);
    let builds = client.builds("paper", "1.21.11").expect("builds");
    assert_eq!(
        builds.len(),
        2,
        "build 33 publishes no server:default download"
    );
    assert_eq!(builds[0].id, 34);
    assert_eq!(builds[0].download.name, "paper-1.21.11-34.jar");
    assert_eq!(
        builds[0].download.sha256,
        "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
        "published checksums are normalized to lowercase"
    );
    assert_eq!(builds[0].download.size, Some(53333322));
    assert_eq!(builds[1].id, 32);
}

#[test]
fn unknown_projects_and_versions_are_typed_http_errors() {
    let server = mock_fill();
    let client = FillClient::new(&server.url);
    match client.versions("ghost") {
        Err(CoreError::Http { status, .. }) => assert_eq!(status, 404),
        other => panic!("expected Http 404, got {other:?}"),
    }
    match client.builds("paper", "9.9.9") {
        Err(CoreError::Http { status, .. }) => assert_eq!(status, 404),
        other => panic!("expected Http 404, got {other:?}"),
    }
}

#[test]
fn transport_failures_are_typed_too() {
    // Bind, learn the port, drop: connections to it are refused.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    drop(listener);
    let client = FillClient::new(&url);
    match client.versions("paper") {
        Err(CoreError::HttpTransport { .. }) => {}
        other => panic!("expected HttpTransport, got {other:?}"),
    }
}

// --- downloader -------------------------------------------------------------

#[test]
fn download_lands_verified_with_no_staging_left() {
    let (server, sha, _) = mock_files();
    let _guard = tempdir::scoped("dl-happy");
    let dir = _guard.path.clone();
    let options = DownloadOptions::new();
    let outcome = download_to_dir(
        &format!("{}/files/paper.jar", server.url),
        &dir,
        "server.jar",
        Some(&sha),
        &options,
    )
    .expect("download");
    assert_eq!(outcome.size, 512 * 1024);
    assert_eq!(outcome.digest, sha);
    assert!(dir.join("server.jar").is_file());
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".zamin-staging") || n.contains(".part"))
        .collect();
    assert!(leftovers.is_empty(), "no staging leftovers: {leftovers:?}");
}

#[test]
fn checksum_mismatch_never_commits() {
    let (server, _, wrong) = mock_files();
    let _guard = tempdir::scoped("dl-checksum");
    let dir = _guard.path.clone();
    let options = DownloadOptions::new();
    let result = download_to_dir(
        &format!("{}/files/paper.jar", server.url),
        &dir,
        "server.jar",
        Some(&wrong),
        &options,
    );
    match result {
        Err(CoreError::ChecksumMismatch {
            expected, actual, ..
        }) => {
            assert_eq!(expected, wrong);
            assert_ne!(actual, wrong);
        }
        other => panic!("expected ChecksumMismatch, got {other:?}"),
    }
    assert!(!dir.join("server.jar").exists(), "the bad bytes never land");
}

#[test]
fn cancellation_stops_mid_stream_and_cleans_up() {
    let (server, sha, _) = mock_files();
    let _guard = tempdir::scoped("dl-cancel");
    let dir = _guard.path.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_progress = Arc::clone(&cancel);
    let mut options = DownloadOptions::new();
    options.cancel = Arc::clone(&cancel);
    options.progress = Some(Arc::new(move |p| {
        if p.bytes_done >= 64 * 1024 {
            cancel_for_progress.store(true, Ordering::Relaxed);
        }
    }));
    let result = download_to_dir(
        &format!("{}/files/slow.jar", server.url),
        &dir,
        "server.jar",
        Some(&sha),
        &options,
    );
    assert!(matches!(result, Err(CoreError::Cancelled)));
    assert!(!dir.join("server.jar").exists());
}

#[test]
fn http_failures_leave_no_file() {
    let (server, _, _) = mock_files();
    let _guard = tempdir::scoped("dl-404");
    let dir = _guard.path.clone();
    let options = DownloadOptions::new();
    let result = download_to_dir(
        &format!("{}/files/nothing.jar", server.url),
        &dir,
        "server.jar",
        None,
        &options,
    );
    match result {
        Err(CoreError::Http { status, .. }) => assert_eq!(status, 404),
        other => panic!("expected Http 404, got {other:?}"),
    }
    assert!(!dir.join("server.jar").exists());
}

#[test]
fn refusing_to_overwrite_an_existing_target() {
    let (server, sha, _) = mock_files();
    let _guard = tempdir::scoped("dl-exists");
    let dir = _guard.path.clone();
    std::fs::write(dir.join("server.jar"), b"precious").unwrap();
    let options = DownloadOptions::new();
    let result = download_to_dir(
        &format!("{}/files/paper.jar", server.url),
        &dir,
        "server.jar",
        Some(&sha),
        &options,
    );
    assert!(result.is_err(), "an existing target is never overwritten");
    assert_eq!(std::fs::read(dir.join("server.jar")).unwrap(), b"precious");
}

// --- templates --------------------------------------------------------------

#[test]
fn default_template_stamps_honest_files() {
    let _guard = tempdir::scoped("tpl-happy");
    let dir = _guard.path.clone();
    assert!(template(DEFAULT_TEMPLATE_ID).is_some());
    assert!(template("marketplace").is_none(), "no fantasy templates");
    assert_eq!(templates().len(), 1, "exactly one honest template in V1");

    let stamped = stamp_template(&dir, DEFAULT_TEMPLATE_ID).expect("stamp");
    assert_eq!(stamped.len(), 2);

    let eula = std::fs::read_to_string(dir.join("eula.txt")).unwrap();
    assert!(
        eula.contains("eula=false"),
        "acceptance is the user's click"
    );
    let props = std::fs::read_to_string(dir.join("server.properties")).unwrap();
    assert!(props.contains("server-port=25565"));
    assert!(
        props.contains("online-mode=true"),
        "auth defaults are never loosened"
    );
}

#[test]
fn stamping_refuses_to_clobber_and_unknown_ids_are_typed() {
    let _guard = tempdir::scoped("tpl-clobber");
    let dir = _guard.path.clone();
    std::fs::write(dir.join("eula.txt"), "eula=true\n").unwrap();
    let result = stamp_template(&dir, DEFAULT_TEMPLATE_ID);
    assert!(result.is_err(), "existing files are never overwritten");
    assert_eq!(
        std::fs::read_to_string(dir.join("eula.txt")).unwrap(),
        "eula=true\n",
        "the user's acceptance is untouched"
    );
    match stamp_template(&dir, "nope") {
        Err(CoreError::NotFound { .. }) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

// --- Fabric meta client (the second family) ----------------------------------

/// The observed meta v2 shapes, newest-first like upstream sends them.
const FABRIC_GAME_JSON: &str = r#"[
  {"version": "1.21.11", "stable": true},
  {"version": "1.21.11-rc3", "stable": false},
  {"version": "26.3-snapshot-3", "stable": false},
  {"version": "1.20.1", "stable": true}
]"#;

const FABRIC_LOADER_JSON: &str = r#"[
  {"url": "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.14/fabric-loader-0.16.14.jar",
   "maven": "net.fabricmc:fabric-loader:0.16.14", "version": "0.16.14", "stable": true},
  {"url": "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.13/fabric-loader-0.16.13.jar",
   "maven": "net.fabricmc:fabric-loader:0.16.13", "version": "0.16.13", "stable": true},
  {"url": "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.17.0-beta.1/fabric-loader-0.17.0-beta.1.jar",
   "maven": "net.fabricmc:fabric-loader:0.17.0-beta.1", "version": "0.17.0-beta.1", "stable": false},
  {"url": "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.15.11/fabric-loader-0.15.11.jar",
   "maven": "net.fabricmc:fabric-loader:0.15.11", "version": "0.15.11", "stable": true}
]"#;

const FABRIC_INSTALLER_JSON: &str = r#"[
  {"url": "https://maven.fabricmc.net/net/fabricmc/fabric-installer/1.0.1/fabric-installer-1.0.1.jar",
   "maven": "net.fabricmc:fabric-installer:1.0.1", "version": "1.0.1", "stable": true},
  {"url": "https://maven.fabricmc.net/net/fabricmc/fabric-installer/1.0.0/fabric-installer-1.0.0.jar",
   "maven": "net.fabricmc:fabric-installer:1.0.0", "version": "1.0.0", "stable": true}
]"#;

const FABRIC_JAR_BYTES: &[u8] = b"PK\x03\x04 fabric launcher jar bytes";

fn mock_fabric() -> MockServer {
    let jar = FABRIC_JAR_BYTES.to_vec();
    MockServer::spawn(move |path| match path {
        "/v2/versions/game" => MockResponse::json(FABRIC_GAME_JSON),
        "/v2/versions/loader" => MockResponse::json(FABRIC_LOADER_JSON),
        "/v2/versions/installer" => MockResponse::json(FABRIC_INSTALLER_JSON),
        "/v2/versions/loader/1.21.11/0.16.14/1.0.1/server/jar" => MockResponse::bytes(jar.clone()),
        _ => MockResponse::not_found(),
    })
}

fn versions(list: &[MetaVersion]) -> Vec<(&str, bool)> {
    list.iter()
        .map(|v| (v.version.as_str(), v.stable))
        .collect()
}

#[test]
fn fabric_version_lists_pass_through_upstream_order() {
    let server = mock_fabric();
    let client = FabricMetaClient::new(&server.url);

    let games = client.game_versions().expect("games");
    assert_eq!(
        versions(&games),
        vec![
            ("1.21.11", true),
            ("1.21.11-rc3", false),
            ("26.3-snapshot-3", false),
            ("1.20.1", true)
        ],
        "the API's newest-first order is kept, stable flags intact"
    );

    let loaders = client.loader_versions().expect("loaders");
    assert_eq!(versions(&loaders)[0], ("0.16.14", true));
    let installers = client.installer_versions().expect("installers");
    assert_eq!(versions(&installers)[0], ("1.0.1", true));
}

#[test]
fn fabric_resolve_defaults_to_the_newest_stable() {
    let server = mock_fabric();
    let client = FabricMetaClient::new(&server.url);

    let jar = client
        .resolve_server_jar("1.21.11", None, None)
        .expect("resolve");
    // The unstable 0.17.0-beta.1 loader exists upstream but stable wins.
    assert_eq!(jar.loader, "0.16.14");
    assert_eq!(jar.installer, "1.0.1");
    assert_eq!(
        jar.name,
        "fabric-server-mc.1.21.11-loader.0.16.14-launcher.1.0.1.jar"
    );
    assert_eq!(
        jar.url,
        format!("{}/v2/versions/loader/1.21.11/0.16.14/1.0.1/server/jar", server.url)
    );
}

#[test]
fn fabric_resolve_pins_explicit_versions() {
    let server = mock_fabric();
    let client = FabricMetaClient::new(&server.url);

    let jar = client
        .resolve_server_jar("1.20.1", Some("0.15.11"), Some("1.0.0"))
        .expect("pinned resolve");
    assert_eq!(jar.loader, "0.15.11");
    assert_eq!(jar.installer, "1.0.0");
    assert_eq!(
        jar.name,
        "fabric-server-mc.1.20.1-loader.0.15.11-launcher.1.0.0.jar"
    );
}

#[test]
fn fabric_resolve_rejects_unknown_pins_and_unstable_defaults_typed() {
    let server = mock_fabric();
    let client = FabricMetaClient::new(&server.url);

    // An unknown game version is a typed 404 before any download.
    match client.resolve_server_jar("9.9.9", None, None) {
        Err(CoreError::Http { status: 404, .. }) => {}
        other => panic!("expected Http 404 for the game, got {other:?}"),
    }
    // An unknown explicit loader likewise.
    match client.resolve_server_jar("1.21.11", Some("0.99.99"), None) {
        Err(CoreError::Http { status: 404, .. }) => {}
        other => panic!("expected Http 404 for the loader, got {other:?}"),
    }
    // An unknown explicit installer likewise.
    match client.resolve_server_jar("1.21.11", None, Some("9.9.9")) {
        Err(CoreError::Http { status: 404, .. }) => {}
        other => panic!("expected Http 404 for the installer, got {other:?}"),
    }

    // A family with no stable loader behind it: the default pick refuses
    // rather than silently pinning a beta.
    let all_unstable = MockServer::spawn(|path| match path {
        "/v2/versions/game" => MockResponse::json(FABRIC_GAME_JSON),
        "/v2/versions/loader" => {
            MockResponse::json(r#"[{"version":"0.17.0-beta.1","stable":false}]"#)
        }
        "/v2/versions/installer" => MockResponse::json(FABRIC_INSTALLER_JSON),
        _ => MockResponse::not_found(),
    });
    let unstable = FabricMetaClient::new(&all_unstable.url);
    match unstable.resolve_server_jar("1.21.11", None, None) {
        Err(CoreError::Http { status: 404, .. }) => {}
        other => panic!("expected Http 404 without a stable loader, got {other:?}"),
    }
}

#[test]
fn fabric_launcher_jar_downloads_through_the_shared_discipline() {
    let server = mock_fabric();
    let client = FabricMetaClient::new(&server.url);
    let resolved = client
        .resolve_server_jar("1.21.11", None, None)
        .expect("resolve");

    let _guard = tempdir::scoped("fabric-jar");
    let dir = _guard.path.clone();
    let options = DownloadOptions::new();
    // No published checksum upstream: `None` expected — the downloader
    // still hashes the stream and reports the digest.
    let outcome = super::download_to_dir(&resolved.url, &dir, "server.jar", None, &options)
        .expect("download");
    assert_eq!(outcome.size, FABRIC_JAR_BYTES.len() as u64);
    assert_eq!(outcome.digest.len(), 64, "a computed sha256 rides along");
    assert_eq!(
        std::fs::read(dir.join("server.jar")).unwrap(),
        FABRIC_JAR_BYTES
    );
}

#[test]
fn fabric_meta_shape_errors_are_typed_transport_errors() {
    // A wrong-shaped response is transport-classified, never a panic.
    let broken = MockServer::spawn(|path| match path {
        "/v2/versions/game" => MockResponse::json(r#"{"unexpected": true}"#),
        _ => MockResponse::not_found(),
    });
    let bad = FabricMetaClient::new(&broken.url);
    match bad.game_versions() {
        Err(CoreError::HttpTransport { message, .. }) => {
            assert!(message.contains("not the expected shape"));
        }
        other => panic!("expected shape error, got {other:?}"),
    }
}
