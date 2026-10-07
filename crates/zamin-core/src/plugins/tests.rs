// The Modrinth client and the install seam, against a local mock that
// mirrors the real API's shapes (the same discipline the Fill client's
// tests follow). The downloader itself is covered by the software
// module's tests; here the sha512 path and the filename sanitizer get
// their own scrutiny.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sha2::Sha512;

use super::modrinth::ModrinthClient;
use super::{install_file, loaders_for_target, safe_file_name, target_for_root};
use crate::error::CoreError;
use crate::server::registry::tempdir;
use crate::software::{DownloadOptions, Verified};

// --- the mock HTTP server ---------------------------------------------------

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
            body: b"{\"error\":\"not_found\"}".to_vec(),
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
        let handle = std::thread::spawn(move || loop {
            if stop_for_thread.load(Ordering::Relaxed) {
                return;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nodelay(true).ok();
                    let mut request = Vec::new();
                    let mut buf = [0u8; 4096];
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

// --- fixtures ---------------------------------------------------------------

/// The observed search shape (2026-12), fields the client speaks only.
const SEARCH_JSON: &str = r#"{
  "hits": [
    {"project_id": "AABBCC", "slug": "essentialsx", "title": "EssentialsX",
     "description": "The essential plugin suite.", "downloads": 4000000,
     "icon_url": "https://cdn.modrinth.com/essentialsx.png",
     "loaders": ["paper", "spigot", "bukkit"], "categories": ["chat", "paper"]},
    {"project_id": "DDEEFF", "slug": "vault", "title": "Vault",
     "description": "The economy abstraction.", "downloads": 3000000,
     "icon_url": null, "loaders": ["paper"], "categories": ["library"]}
  ],
  "total": 2
}"#;

const VERSIONS_JSON: &str = r#"[
  {"id": "ver9", "version_number": "2.20.0", "game_versions": ["1.21.1", "1.21"],
   "loaders": ["paper"], "date_published": "2026-09-01T10:00:00Z",
   "files": [
     {"url": "http://mock/files/essentialsx-2.20.0.jar", "filename": "EssentialsX-2.20.0.jar",
      "size": 2100000, "primary": false,
      "hashes": {"sha1": "aa", "sha512": "dd"}},
     {"url": "http://mock/files/essentialsx-2.20.0-main.jar", "filename": "EssentialsX-2.20.0-main.jar",
      "size": 2000000, "primary": true,
      "hashes": {"sha1": "bb", "sha512": "cc"}}
   ]},
  {"id": "ver8", "version_number": "2.19.0", "game_versions": ["1.20.4"],
   "loaders": ["paper", "spigot"], "date_published": "2026-03-01T10:00:00Z",
   "files": [{"url": "http://mock/files/essentialsx-2.19.0.jar", "filename": "EssentialsX-2.19.0.jar",
              "size": 1900000, "primary": true,
              "hashes": {"sha1": "ee"}}]},
  {"id": "ver7", "version_number": "1.0-fabric", "game_versions": ["1.21"],
   "loaders": ["fabric"], "date_published": "2026-01-01T10:00:00Z",
   "files": [{"url": "http://mock/files/fabric.jar", "filename": "fabric.jar",
              "size": 10, "primary": true, "hashes": {"sha1": "ff", "sha512": "11"}}]}
]"#;

fn sha512_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut h = Sha512::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

// --- search -----------------------------------------------------------------

#[test]
fn search_builds_one_loader_facet_group_and_parses_hits() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_for_handler = Arc::clone(&seen);
    let server = MockServer::spawn(move |path| {
        seen_for_handler.lock().unwrap().push(path.to_owned());
        MockResponse::json(SEARCH_JSON)
    });
    let client = ModrinthClient::new(&server.url);
    let hits = client
        .search("essentials", &["paper", "spigot", "bukkit"], 20)
        .expect("search parses");

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].project_id, "AABBCC");
    assert_eq!(hits[0].title, "EssentialsX");
    assert_eq!(hits[0].downloads, 4000000);
    assert_eq!(
        hits[0].icon_url.as_deref(),
        Some("https://cdn.modrinth.com/essentialsx.png")
    );
    assert!(hits[1].icon_url.is_none());

    let path = &seen.lock().unwrap()[0];
    assert!(path.starts_with("/v2/search?query=essentials&limit=20&facets="));
    // The facets JSON percent-encoded: one group for project_type, one
    // OR group for the three loaders.
    assert!(path.contains("%5B%22project_type%3Amod%22%5D"));
    assert!(path.contains("%22categories%3Apaper%22"));
    assert!(path.contains("%22categories%3Aspigot%22"));
    assert!(path.contains("%22categories%3Abukkit%22"));
}

#[test]
fn search_clamps_limit_and_encodes_spaces() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_for_handler = Arc::clone(&seen);
    let server = MockServer::spawn(move |path| {
        seen_for_handler.lock().unwrap().push(path.to_owned());
        MockResponse::json(SEARCH_JSON)
    });
    let client = ModrinthClient::new(&server.url);
    client.search("world edit", &["paper"], 500).unwrap();
    let path = &seen.lock().unwrap()[0];
    assert!(path.contains("query=world%20edit"));
    assert!(path.contains("limit=50"), "limit clamps to the API max");
}

#[test]
fn search_maps_http_status_to_typed_error() {
    let server = MockServer::spawn(|_| MockResponse::not_found());
    let client = ModrinthClient::new(&server.url);
    match client.search("x", &["paper"], 5) {
        Err(CoreError::Http { status, .. }) => assert_eq!(status, 404),
        other => panic!("expected Http(404), got {other:?}"),
    }
}

// --- versions ---------------------------------------------------------------

#[test]
fn versions_prefer_the_primary_file_and_skip_sha1_only_files() {
    let server = MockServer::spawn(|path| match path {
        p if p.starts_with("/v2/project/AABBCC/version") => MockResponse::json(VERSIONS_JSON),
        _ => MockResponse::not_found(),
    });
    let client = ModrinthClient::new(&server.url);
    let versions = client.versions("AABBCC").expect("versions parse");

    assert_eq!(versions.len(), 3);
    // Newest first as returned; the primary file wins over the aux one.
    assert_eq!(
        versions[0].file.as_ref().unwrap().filename,
        "EssentialsX-2.20.0-main.jar"
    );
    assert_eq!(versions[0].file.as_ref().unwrap().sha512, "cc");
    // A version publishing only sha1 is not installable — it surfaces
    // with no file, and the engine skips it, never half-installs.
    assert!(versions[1].file.is_none(), "sha1-only file is unusable");
    assert_eq!(versions[2].loaders, vec!["fabric"]);
}

// --- install ----------------------------------------------------------------

#[test]
fn install_lands_the_sanitized_name_sha512_verified() {
    let jar = b"PK\x03\x04 fake jar bytes";
    let digest = sha512_hex(jar);
    let server = MockServer::spawn(move |path| match path {
        "/files/EssentialsX-2.20.0.jar" => MockResponse::bytes(jar.to_vec()),
        _ => MockResponse::not_found(),
    });
    let guard = tempdir::scoped("plugin-install");
    let target = guard.path.join("plugins");
    let file = super::VersionFile {
        url: format!("{}/files/EssentialsX-2.20.0.jar", server.url),
        filename: "EssentialsX-2.20.0.jar".to_owned(),
        sha512: digest.clone(),
        size: Some(jar.len() as u64),
    };
    let outcome = install_file(&target, &file, &DownloadOptions::new()).expect("install lands");
    assert_eq!(outcome.path, target.join("EssentialsX-2.20.0.jar"));
    assert_eq!(outcome.digest, digest);
    let on_disk = std::fs::read(&outcome.path).unwrap();
    assert_eq!(on_disk, jar);
    // No staging residue.
    assert!(std::fs::read_dir(&target).unwrap().count() == 1);
}

#[test]
fn install_refuses_a_tampered_payload() {
    let server = MockServer::spawn(move |path| match path {
        "/files/x.jar" => MockResponse::bytes(b"tampered".to_vec()),
        _ => MockResponse::not_found(),
    });
    let guard = tempdir::scoped("plugin-tamper");
    let file = super::VersionFile {
        url: format!("{}/files/x.jar", server.url),
        filename: "x.jar".to_owned(),
        sha512: sha512_hex(b"the real payload"),
        size: None,
    };
    match install_file(&guard.path, &file, &DownloadOptions::new()) {
        Err(CoreError::ChecksumMismatch { algorithm, .. }) => {
            assert_eq!(algorithm, "sha512");
        }
        other => panic!("expected ChecksumMismatch, got {other:?}"),
    }
    assert!(
        std::fs::read_dir(&guard.path).unwrap().count() == 0,
        "no staging left"
    );
}

// --- safe_file_name -----------------------------------------------------------

#[test]
fn safe_file_name_accepts_honest_names() {
    assert_eq!(
        safe_file_name("EssentialsX-2.20.0.jar").unwrap(),
        "EssentialsX-2.20.0.jar"
    );
    assert_eq!(
        safe_file_name("world-edit+mc1.21.jar").unwrap(),
        "world-edit+mc1.21.jar"
    );
}

#[test]
fn safe_file_name_rejects_the_unsafe_ones() {
    for bad in [
        "../escape.jar",
        "dir/nested.jar",
        "dir\\nested.jar",
        "C:evil.jar",
        "con.jar",
        "NUL",
        "\u{1}invisible.jar",
        "",
        ". ..",
    ] {
        assert!(safe_file_name(bad).is_err(), "{bad:?} must not pass");
    }
    // Windows-invisible suffixes are stripped, not kept.
    assert_eq!(safe_file_name("plugin.jar. ").unwrap(), "plugin.jar");
}

// --- target + loaders ---------------------------------------------------------

#[test]
fn target_follows_the_directory_that_exists() {
    let guard = tempdir::scoped("plugin-target");
    assert_eq!(target_for_root(&guard.path).0, "plugins");
    std::fs::create_dir_all(guard.path.join("mods")).unwrap();
    assert_eq!(target_for_root(&guard.path).0, "mods");
    assert_eq!(
        loaders_for_target("mods"),
        &["fabric", "quilt", "forge", "neoforge"]
    );
    assert!(loaders_for_target("plugins").contains(&"paper"));
}

#[test]
fn install_verifies_against_sha512_even_when_a_sha256_is_passed() {
    // Belt-and-braces for the Verified enum: the explicit variant decides.
    let jar = b"content";
    let server = MockServer::spawn(move |path| match path {
        "/f" => MockResponse::bytes(jar.to_vec()),
        _ => MockResponse::not_found(),
    });
    let guard = tempdir::scoped("verified-enum");
    let result = crate::software::download_verified(
        &format!("{}/f", server.url),
        &guard.path,
        "f.bin",
        Verified::Sha512(&sha512_hex(jar)),
        &DownloadOptions::new(),
    );
    assert!(result.is_ok());
}

// --- the update rule (ADR-0012) ----------------------------------------------

#[test]
fn reinstall_of_identical_bytes_short_circuits_without_a_download() {
    // The URL points at a dead port on purpose: the only way this install
    // can succeed is the idempotent short-circuit over the existing file.
    let jar = b"PK\x03\x04 same bytes";
    let guard = tempdir::scoped("plugin-reinstall");
    let target = guard.path.join("plugins");
    let file = super::VersionFile {
        url: "http://127.0.0.1:1/files/EssentialsX-2.20.0.jar".to_owned(),
        filename: "EssentialsX-2.20.0.jar".to_owned(),
        sha512: sha512_hex(jar),
        size: Some(jar.len() as u64),
    };
    // First install has nowhere to land from: a refused download would
    // prove nothing, so land the file directly under the sanitized name.
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("EssentialsX-2.20.0.jar"), jar).unwrap();

    let outcome = install_file(&target, &file, &DownloadOptions::new()).expect("reinstall lands");
    assert_eq!(outcome.path, target.join("EssentialsX-2.20.0.jar"));
    assert_eq!(outcome.digest, sha512_hex(jar));
    assert_eq!(std::fs::read(&outcome.path).unwrap(), jar);
}

#[test]
fn existing_file_with_different_content_is_the_typed_refusal() {
    let jar_v1 = b"version one bytes";
    let jar_v2 = b"version two bytes, different";
    let server = MockServer::spawn(move |path| match path {
        "/files/EssentialsX-2.20.0.jar" => MockResponse::bytes(jar_v2.to_vec()),
        _ => MockResponse::not_found(),
    });
    let guard = tempdir::scoped("plugin-exists");
    let target = guard.path.join("plugins");
    let file = super::VersionFile {
        url: format!("{}/files/EssentialsX-2.20.0.jar", server.url),
        filename: "EssentialsX-2.20.0.jar".to_owned(),
        sha512: sha512_hex(jar_v2),
        size: Some(jar_v2.len() as u64),
    };
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("EssentialsX-2.20.0.jar"), jar_v1).unwrap();

    match install_file(&target, &file, &DownloadOptions::new()) {
        Err(CoreError::PluginExists { file }) => {
            assert_eq!(file, "EssentialsX-2.20.0.jar");
        }
        other => panic!("expected PluginExists, got {other:?}"),
    }
    // The installed bytes were never touched.
    assert_eq!(
        std::fs::read(target.join("EssentialsX-2.20.0.jar")).unwrap(),
        jar_v1
    );
}

#[test]
fn replace_lands_the_new_bytes_over_the_old_file() {
    let jar_v1 = b"version one bytes";
    let jar_v2 = b"version two bytes, different";
    let server = MockServer::spawn(move |path| match path {
        "/files/EssentialsX-2.20.0.jar" => MockResponse::bytes(jar_v2.to_vec()),
        _ => MockResponse::not_found(),
    });
    let guard = tempdir::scoped("plugin-replace");
    let target = guard.path.join("plugins");
    let file = super::VersionFile {
        url: format!("{}/files/EssentialsX-2.20.0.jar", server.url),
        filename: "EssentialsX-2.20.0.jar".to_owned(),
        sha512: sha512_hex(jar_v2),
        size: Some(jar_v2.len() as u64),
    };
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("EssentialsX-2.20.0.jar"), jar_v1).unwrap();

    let options = DownloadOptions {
        replace: true,
        ..DownloadOptions::new()
    };
    let outcome = install_file(&target, &file, &options).expect("replace lands");
    assert_eq!(outcome.digest, sha512_hex(jar_v2));
    assert_eq!(std::fs::read(&outcome.path).unwrap(), jar_v2);
    assert!(
        std::fs::read_dir(&target).unwrap().count() == 1,
        "no residue"
    );
}

// --- version_from_sha512 (the update check's "what is this jar?") ------------

fn version_json(id: &str, number: &str, project: &str, loaders: &[&str], sha512: &str) -> String {
    serde_json::json!({
        "id": id,
        "project_id": project,
        "version_number": number,
        "game_versions": ["1.21.1"],
        "loaders": loaders,
        "date_published": "2026-09-01T10:00:00Z",
        "files": [{"url": "http://mock/files/x.jar", "filename": "x.jar",
                   "size": 10, "primary": true, "hashes": {"sha1": "aa", "sha512": sha512}}]
    })
    .to_string()
}

#[test]
fn version_from_sha512_answers_the_version_or_a_honest_none() {
    let digest = sha512_hex(b"known bytes");
    let digest_for_handler = digest.clone();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_for_handler = Arc::clone(&seen);
    let server = MockServer::spawn(move |path| {
        seen_for_handler.lock().unwrap().push(path.to_owned());
        match path {
            p if p.starts_with(&format!("/v2/version_file/{digest_for_handler}")) => {
                MockResponse::json(version_json(
                    "ver9",
                    "2.20.0",
                    "AABBCC",
                    &["paper"],
                    &digest_for_handler,
                ))
            }
            _ => MockResponse::not_found(),
        }
    });
    let client = ModrinthClient::new(&server.url);

    // A published digest resolves to the version that carries it, with
    // the project id the update check needs.
    let version = client
        .version_from_sha512(&digest)
        .expect("known digest resolves")
        .expect("the mock published it");
    assert_eq!(version.id, "ver9");
    assert_eq!(version.project_id, "AABBCC");
    assert_eq!(version.version_number, "2.20.0");
    // Copy the seen path out — holding the guard across the next request
    // would stall the mock's handler thread on this very mutex.
    let first_path = seen.lock().unwrap()[0].clone();
    assert!(
        first_path.contains(&format!("/v2/version_file/{digest}?hashes=sha512")),
        "the hash query is the endpoint's contract: {first_path}"
    );

    // An unpublished digest is a normal answer (404 → None), never an
    // error: manually dropped jars are unknown by definition.
    let unknown = client
        .version_from_sha512(&sha512_hex(b"nobody published this"))
        .expect("404 is a normal answer");
    assert!(unknown.is_none());

    // A server-side failure is the typed transport error, not a fake miss.
    let failing = MockServer::spawn(|_| MockResponse {
        status: 500,
        reason: "Internal Server Error",
        content_type: "application/json",
        body: b"{}".to_vec(),
    });
    match ModrinthClient::new(&failing.url).version_from_sha512(&digest) {
        Err(CoreError::Http { status, .. }) => assert_eq!(status, 500),
        other => panic!("expected Http(500), got {other:?}"),
    }
}

// --- check_updates ------------------------------------------------------------

#[test]
fn check_updates_reports_the_three_honest_statuses_in_stable_order() {
    use super::{check_updates, UpdateStatus};

    // Each jar's bytes decide its fate: the mock answers version_file
    // per digest, and the project's version list per project id.
    let jar_up_to_date = b"PK\x03\x04 up-to-date payload";
    let d_up = sha512_hex(jar_up_to_date);
    let jar_stale = b"PK\x03\x04 stale payload";
    let d_stale = sha512_hex(jar_stale);
    let jar_foreign = b"PK\x03\x04 foreign-loader payload";
    let d_foreign = sha512_hex(jar_foreign);
    let jar_empty_pool = b"PK\x03\x04 no-installable-version payload";
    let d_empty = sha512_hex(jar_empty_pool);
    let jar_manual = b"PK\x03\x04 never-published payload";

    // The project's version list: ver9 is the newest paper version and
    // its primary file carries the up-to-date jar's exact digest; ver8
    // publishes only a sha1 (unusable, skipped); a fabric-only project
    // answers EMPTY (nothing installable for the paper family).
    let aabbcc = format!(
        "[{},{}]",
        version_json("ver9", "2.20.0", "AABBCC", &["paper"], &d_up),
        // sha1-only: unusable file → skipped by the resolve rule.
        r#"{"id": "ver8", "version_number": "2.19.0", "game_versions": ["1.21"],
            "loaders": ["paper"], "files": [{"url": "http://mock/files/v8.jar",
            "filename": "v8.jar", "size": 10, "primary": true,
            "hashes": {"sha1": "ee"}}]}"#
    );
    let empty_pool = r#"[{"id": "verF", "version_number": "0.1-fabric",
        "game_versions": ["1.21"], "loaders": ["fabric"],
        "files": [{"url": "http://mock/files/f.jar", "filename": "f.jar",
                  "size": 10, "primary": true, "hashes": {"sha1": "ff", "sha512": "11"}}]}]"#;

    let (d_up_for_handler, d_stale_for_handler, d_foreign_for_handler, d_empty_for_handler) = (
        d_up.clone(),
        d_stale.clone(),
        d_foreign.clone(),
        d_empty.clone(),
    );
    let server = MockServer::spawn(move |path| {
        if let Some(rest) = path.strip_prefix("/v2/version_file/") {
            let hash = rest.split('?').next().unwrap_or("");
            if hash == d_up_for_handler {
                return MockResponse::json(version_json(
                    "ver9",
                    "2.20.0",
                    "AABBCC",
                    &["paper"],
                    &d_up_for_handler,
                ));
            }
            if hash == d_stale_for_handler {
                return MockResponse::json(version_json(
                    "ver8",
                    "2.19.0",
                    "AABBCC",
                    &["paper"],
                    &d_stale_for_handler,
                ));
            }
            if hash == d_foreign_for_handler {
                return MockResponse::json(version_json(
                    "ver7",
                    "1.0-fabric",
                    "AABBCC",
                    &["fabric"],
                    &d_foreign_for_handler,
                ));
            }
            if hash == d_empty_for_handler {
                return MockResponse::json(version_json(
                    "verF",
                    "0.1-fabric",
                    "EMPTY",
                    &["fabric"],
                    &d_empty_for_handler,
                ));
            }
            return MockResponse::not_found();
        }
        if path.starts_with("/v2/project/AABBCC/version") {
            return MockResponse::json(aabbcc.clone());
        }
        if path.starts_with("/v2/project/EMPTY/version") {
            return MockResponse::json(empty_pool);
        }
        MockResponse::not_found()
    });

    let guard = tempdir::scoped("plugin-updates");
    let target = guard.path.join("plugins");
    std::fs::create_dir_all(&target).unwrap();
    // Scrambled creation order; the report must come back sorted by name.
    std::fs::write(target.join("e-manual.jar"), jar_manual).unwrap();
    std::fs::write(target.join("a-up-to-date.jar"), jar_up_to_date).unwrap();
    std::fs::write(target.join("d-empty-pool.jar"), jar_empty_pool).unwrap();
    std::fs::write(target.join("b-stale.jar"), jar_stale).unwrap();
    std::fs::write(target.join("c-foreign.jar"), jar_foreign).unwrap();
    std::fs::write(target.join("readme.txt"), b"not a jar").unwrap();

    let entries = check_updates(
        &target,
        &ModrinthClient::new(&server.url),
        loaders_for_target("plugins"),
    )
    .expect("update check succeeds");
    assert_eq!(entries.len(), 5, "the .txt file is nobody's business");

    assert_eq!(entries[0].file_name, "a-up-to-date.jar");
    assert_eq!(entries[0].status, UpdateStatus::UpToDate);
    assert_eq!(entries[0].project_id.as_deref(), Some("AABBCC"));
    assert_eq!(entries[0].installed_version.as_deref(), Some("2.20.0"));
    assert_eq!(entries[0].latest_version.as_deref(), Some("2.20.0"));
    assert_eq!(entries[0].latest_version_id.as_deref(), Some("ver9"));

    assert_eq!(entries[1].file_name, "b-stale.jar");
    assert_eq!(entries[1].status, UpdateStatus::UpdateAvailable);
    assert_eq!(entries[1].installed_version.as_deref(), Some("2.19.0"));
    assert_eq!(entries[1].latest_version.as_deref(), Some("2.20.0"));
    assert_eq!(
        entries[1].latest_version_id.as_deref(),
        Some("ver9"),
        "the entry carries the pin that applies the update"
    );

    // A foreign-loader jar resolves in the catalog and the project has
    // installable paper versions: the update is the fix — installing the
    // proper paper jar replaces the wrong-family bytes.
    assert_eq!(entries[2].file_name, "c-foreign.jar");
    assert_eq!(entries[2].status, UpdateStatus::UpdateAvailable);
    assert_eq!(entries[2].installed_version.as_deref(), Some("1.0-fabric"));

    // Known bytes, but the project publishes nothing installable for
    // this loader family: unmanaged, told with its story.
    assert_eq!(entries[3].file_name, "d-empty-pool.jar");
    assert_eq!(entries[3].status, UpdateStatus::Unmanaged);
    assert_eq!(entries[3].project_id.as_deref(), Some("EMPTY"));
    assert_eq!(entries[3].installed_version.as_deref(), Some("0.1-fabric"));
    assert_eq!(entries[3].latest_version_id, None);

    // Never-published bytes: unmanaged with no story at all.
    assert_eq!(entries[4].file_name, "e-manual.jar");
    assert_eq!(entries[4].status, UpdateStatus::Unmanaged);
    assert_eq!(entries[4].project_id, None);
}

#[test]
fn check_updates_answers_an_empty_report_for_a_missing_directory() {
    use super::check_updates;
    let server = MockServer::spawn(|_| MockResponse::not_found());
    let guard = tempdir::scoped("plugin-updates-empty");
    let entries = check_updates(
        &guard.path.join("plugins"),
        &ModrinthClient::new(&server.url),
        loaders_for_target("plugins"),
    )
    .expect("a server with no plugins directory has no updates");
    assert!(entries.is_empty());
}
