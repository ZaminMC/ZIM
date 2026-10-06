//! `server.create` + the catalog surface (protocol spec §7b): the New
//! Server flow over the wire — catalog browsing, a download job with
//! progress and cancellation, typed failures that leave nothing behind,
//! and the full zero-manual-JAR journey ending in a running server.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use common::{
    accept_eula, connect_daemon, fake_server_exe, scoped_dir, spawn_daemon_with, subscribe_events,
    wait_for_state, wait_list_state, MockHttp, MockHttpResponse,
};
use zamin_protocol::methods;

/// The mock speaks the Fill API's observed shapes for project `paper`.
/// The build's download URL must be absolute, and only the spawned mock
/// knows its own address — the handler reads it from a cell filled right
/// after spawn, before any request can arrive.
fn spawn_catalog(jar: Vec<u8>, drip_ms: u64) -> (MockHttp, String) {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&jar);
    let sha: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mock = spawn_catalog_with_published_sha(jar, &sha, drip_ms);
    (mock, sha)
}

/// The published sha may deliberately lie to the downloader (that is what
/// the checksum-mismatch test needs).
fn spawn_catalog_with_published_sha(jar: Vec<u8>, published_sha: &str, drip_ms: u64) -> MockHttp {
    let sha = published_sha.to_owned();
    let base: Arc<std::sync::OnceLock<String>> = Arc::new(std::sync::OnceLock::new());
    let base_for_handler = Arc::clone(&base);
    let mock = MockHttp::spawn(move |path| match path {
        "/projects/paper" => MockHttpResponse::json(
            r#"{"project":{"id":"paper","name":"Paper"},"versions":{"1.21":["1.21.11","1.21.9"],"1.20":["1.20.4"]}}"#,
        ),
        "/projects/paper/versions/1.21.11" => MockHttpResponse::json(
            r#"{"version":{"id":"1.21.11","java":{"version":{"minimum":21}}},"builds":[34]}"#,
        ),
        "/projects/paper/versions/1.21.11/builds" => {
            let base = base_for_handler.get().cloned().unwrap_or_default();
            let body = serde_json::to_vec(&json!([{
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
            .unwrap();
            MockHttpResponse::json(body)
        }
        "/projects/paper/versions/9.9.9/builds" => MockHttpResponse::not_found(),
        "/projects/ghost" => MockHttpResponse::not_found(),
        "/files/paper.jar" => {
            let response = MockHttpResponse::bytes(jar.clone());
            if drip_ms > 0 {
                response.dripping(drip_ms)
            } else {
                response
            }
        }
        _ => MockHttpResponse::not_found(),
    });
    let _ = base.set(mock.url.clone());
    mock
}

/// A distinct endpoint per test: the tag is hashed with the pid, so two
/// tests sharing a tag share a socket — give each test its own.
fn unique_endpoint() -> zamin_ipc::Endpoint {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    zamin_ipc::Endpoint::unique_for_test(&format!("create-{}", N.fetch_add(1, Ordering::Relaxed)))
}

fn create_params(server_id: &str, build: Value) -> Value {
    json!({
        "requestId": uuid::Uuid::now_v7().to_string(),
        "serverId": server_id,
        "displayName": "Created Server",
        "project": "paper",
        "version": "1.21.11",
        "build": build,
    })
}

#[tokio::test]
async fn create_downloads_verifies_and_registers() {
    let jar: Vec<u8> = (0..512 * 1024u32).map(|i| (i % 251) as u8).collect();
    let (catalog, _jar_sha) = spawn_catalog(jar.clone(), 0);
    let data_dir = scoped_dir("create-happy");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    let result = client
        .request(methods::SERVER_CREATE, create_params("created", json!(34)))
        .await
        .expect("server.create");
    let job_id = result["job"]["jobId"].as_str().expect("jobId").to_owned();
    assert_eq!(result["kind"], "server.create");
    assert_eq!(result["job"]["state"], "running");

    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded", "error: {error:?}");

    // The server is registered and listable.
    wait_list_state(
        &mut client,
        "created",
        "not-running",
        Duration::from_secs(10),
    )
    .await;

    // Files on disk: verified jar bytes + template stamps.
    let root = data_dir.join("instances").join("created");
    assert_eq!(std::fs::read(root.join("server.jar")).unwrap(), jar);
    let eula = std::fs::read_to_string(root.join("eula.txt")).unwrap();
    assert!(
        eula.contains("eula=false"),
        "acceptance stays the user's click"
    );
    let props = std::fs::read_to_string(root.join("server.properties")).unwrap();
    assert!(props.contains("server-port=25565"));

    // Per-server configuration carries the catalog's knowledge.
    let config = std::fs::read_to_string(data_dir.join("servers/created/config.toml")).unwrap();
    assert!(config.contains("mcVersion = \"1.21.11\""), "{config}");
    assert!(config.contains("javaMajorRequired = 21"), "{config}");
    assert!(config.contains("jar = \"server.jar\""), "{config}");
    assert!(
        config.contains("displayName = \"Created Server\""),
        "{config}"
    );

    // No staging leftovers in the instance dir.
    let leftovers: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".zamin-staging") || n.ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[tokio::test]
async fn create_defaults_to_the_newest_build() {
    let jar: Vec<u8> = vec![7u8; 64 * 1024];
    let (catalog, _jar_sha) = spawn_catalog(jar, 0);
    let data_dir = scoped_dir("create-latest");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    let params = create_params("latest-built", json!(null));
    let result = client
        .request(methods::SERVER_CREATE, params)
        .await
        .expect("server.create without build");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded", "{error:?}");
    assert!(data_dir.join("instances/latest-built/server.jar").is_file());
}

#[tokio::test]
async fn checksum_mismatch_fails_typed_and_cleans_up() {
    // The mock serves bytes that do not hash to SHA_OK.
    let body = vec![1u8; 128 * 1024];
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&body);
    let real_sha: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let published = format!("{:0>64}", "e"); // a lie, but well-formed
    assert_ne!(published, real_sha);
    let catalog = spawn_catalog_with_published_sha(body, &published, 0);
    let data_dir = scoped_dir("create-checksum");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    let result = client
        .request(methods::SERVER_CREATE, create_params("tampered", json!(34)))
        .await
        .expect("server.create accepted");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "failed");
    let error = error.expect("typed error");
    assert_eq!(error["code"], "CHECKSUM_MISMATCH", "{error}");

    // Nothing appeared: not registered, no directory, no marker.
    let list = client
        .request(methods::SERVER_LIST, json!({}))
        .await
        .unwrap();
    assert!(
        !list["servers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["serverId"] == "tampered"),
        "a failed creation must not register"
    );
    assert!(!data_dir.join("instances/tampered").exists());
}

#[tokio::test]
async fn cancelling_creation_leaves_nothing_behind() {
    // Drip the 8 MiB body in 64 KiB chunks, 30 ms apart: the download
    // cannot finish before the cancel request lands.
    let (catalog, _jar_sha) = spawn_catalog(vec![9u8; 8 * 1024 * 1024], 30);
    let data_dir = scoped_dir("create-cancel");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    let result = client
        .request(methods::SERVER_CREATE, create_params("doomed", json!(34)))
        .await
        .expect("server.create accepted");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    // Give the job a moment to enter the download, then cancel.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let cancelled = client
        .request(
            methods::JOBS_CANCEL,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "jobId": job_id}),
        )
        .await
        .expect("jobs.cancel");
    assert_eq!(cancelled["state"], "running");

    let (state, _) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "cancelled");
    assert!(
        !data_dir.join("instances/doomed").exists(),
        "cancelled creation cleans up"
    );
}

#[tokio::test]
async fn bad_requests_are_typed_before_any_job() {
    let (catalog, _jar_sha) = spawn_catalog(vec![0u8; 1024], 0);
    let data_dir = scoped_dir("create-typed");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    // Unknown project → CATALOG_NOT_FOUND, synchronously.
    let mut params = create_params("ghosty", json!(34));
    params["project"] = json!("ghost");
    let error = client
        .request(methods::SERVER_CREATE, params)
        .await
        .expect_err("unknown project is a typed rejection");
    assert_eq!(error["code"], "CATALOG_NOT_FOUND", "{error}");

    // Unknown version → CATALOG_NOT_FOUND (404 from the builds endpoint).
    let mut params = create_params("versionless", json!(34));
    params["version"] = json!("9.9.9");
    let error = client
        .request(methods::SERVER_CREATE, params)
        .await
        .expect_err("unknown version is a typed rejection");
    assert_eq!(error["code"], "CATALOG_NOT_FOUND", "{error}");

    // Unknown template → PROTOCOL_INVALID_REQUEST.
    let mut params = create_params("templated", json!(34));
    params["templateId"] = json!("marketplace");
    let error = client
        .request(methods::SERVER_CREATE, params)
        .await
        .expect_err("unknown template is a typed rejection");
    assert_eq!(error["code"], "PROTOCOL_INVALID_REQUEST", "{error}");

    // Catalog browsing reflects the same honesty.
    let error = client
        .request(methods::CATALOG_VERSIONS, json!({"project": "ghost"}))
        .await
        .expect_err("catalog.versions for an unknown project");
    assert_eq!(error["code"], "CATALOG_NOT_FOUND");
}

#[tokio::test]
async fn duplicate_ids_are_rejected() {
    let (catalog, _jar_sha) = spawn_catalog(vec![0u8; 64 * 1024], 0);
    let data_dir = scoped_dir("create-dup");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    let result = client
        .request(methods::SERVER_CREATE, create_params("twice", json!(34)))
        .await
        .expect("first create");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, _) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded");

    let error = client
        .request(methods::SERVER_CREATE, create_params("twice", json!(34)))
        .await
        .expect_err("second create with the same id");
    assert_eq!(error["code"], "SERVER_ID_EXISTS", "{error}");
}

#[tokio::test]
async fn catalog_surface_browses_the_mock() {
    let (catalog, jar_sha) = spawn_catalog(vec![0u8; 16], 0);
    let data_dir = scoped_dir("catalog-browse");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    let list = client
        .request(methods::CATALOG_LIST, json!({}))
        .await
        .expect("catalog.list");
    let ids: Vec<&str> = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"paper"));

    let versions = client
        .request(methods::CATALOG_VERSIONS, json!({"project": "paper"}))
        .await
        .expect("catalog.versions");
    let ids: Vec<&str> = versions["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["1.21.11", "1.21.9", "1.20.4"], "newest first");

    let builds = client
        .request(
            methods::CATALOG_BUILDS,
            json!({"project": "paper", "version": "1.21.11"}),
        )
        .await
        .expect("catalog.builds");
    assert_eq!(
        builds["javaMajor"], 21,
        "the version's own requirement rides along"
    );
    assert_eq!(builds["builds"][0]["id"], 34);
    assert_eq!(builds["builds"][0]["channel"], "DEFAULT");
    assert_eq!(
        builds["builds"][0]["download"]["name"],
        "paper-1.21.11-34.jar"
    );
    assert_eq!(builds["builds"][0]["download"]["sha256"], jar_sha);
}

/// THE proof of Phase 6: create from the catalog → the daemon's typed
/// NEEDS_EULA on first start → accept through the file surface → run.
/// Zero manual JAR handling anywhere in the journey.
#[tokio::test]
async fn created_server_runs_after_eula_acceptance() {
    let jar: Vec<u8> = (0..256 * 1024u32).map(|i| (i % 7) as u8).collect();
    let (catalog, _jar_sha) = spawn_catalog(jar, 0);
    let data_dir = scoped_dir("create-journey");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;
    subscribe_events(&mut client, Some("journey")).await;

    // A distinct port: parallel runs and orphans never contend for 25565.
    let mut params = create_params("journey", json!(34));
    params["port"] = json!(26100);
    let result = client
        .request(methods::SERVER_CREATE, params)
        .await
        .expect("server.create");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, _) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded");

    // Point the created server's java at the fake-mc-server binary (the
    // catalog's paper jar is fixture bytes; the runtime is a test seam).
    let java_path = fake_server_exe();
    let java_path_str = java_path.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        data_dir.join("servers/journey/config.toml"),
        format!(
            "[settings]\njavaPath = \"{java_path_str}\"\nmcVersion = \"1.21.11\"\njavaMajorRequired = 21\n"
        ),
    )
    .unwrap();

    // First start: the honest gate.
    let error = client
        .request(
            methods::SERVER_START,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "journey"}),
        )
        .await
        .expect_err("a freshly created server has not accepted the EULA");
    assert_eq!(error["code"], "NEEDS_EULA", "{error}");
    assert_eq!(error["remediation"][0], "accept_eula");

    // The panel's acceptance affordance is a file write — the same move.
    accept_eula(&mut client, "journey").await;

    // Second start runs.
    client
        .request(
            methods::SERVER_START,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "journey"}),
        )
        .await
        .expect("start after acceptance");
    let running = wait_for_state(
        &mut client,
        serde_json::from_value(json!("running")).unwrap(),
        Duration::from_secs(30),
    )
    .await;
    assert!(
        running.is_some(),
        "the created server never reached running"
    );

    // Leave it stopped before the daemon drops (harness tree-kills).
    let _ = client
        .request(
            methods::SERVER_KILL,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "serverId": "journey"}),
        )
        .await;
}

#[tokio::test]
async fn port_choice_lands_in_properties_and_settings() {
    let (catalog, _jar_sha) = spawn_catalog(vec![0u8; 1024], 0);
    let data_dir = scoped_dir("create-port");
    let endpoint = unique_endpoint();
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--catalog-url", &catalog.url]);
    let mut client = connect_daemon(&endpoint).await;

    let mut params = create_params("ported", json!(34));
    params["port"] = json!(25600);
    let result = client
        .request(methods::SERVER_CREATE, params)
        .await
        .expect("server.create with port");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, error) = MockHttp::wait_job(&mut client, &job_id, Duration::from_secs(30)).await;
    assert_eq!(state, "succeeded", "{error:?}");

    let props =
        std::fs::read_to_string(data_dir.join("instances/ported/server.properties")).unwrap();
    assert!(props.contains("server-port=25600"), "{props}");
    let config = std::fs::read_to_string(data_dir.join("servers/ported/config.toml")).unwrap();
    assert!(config.contains("port = 25600"), "{config}");
}
