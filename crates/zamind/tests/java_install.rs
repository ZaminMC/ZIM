//! `java.list` + `java.install` (protocol spec §7c): the daemon fetches a
//! JDK from a (mocked) Adoptium API, verifies, extracts, inspects, and
//! serves it as a managed runtime. The happy path runs on unix only —
//! the fake `java` inside the fixture archive is a shell script — while
//! the cancel path is platform-neutral (it never reaches extraction).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Write as _;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use common::{connect_daemon, scoped_dir, spawn_daemon_with, MockHttp, MockHttpResponse};
use zamin_protocol::methods;

/// A fake JDK: `jdk-<release>/bin/java` as a shell script answering the
/// inspection probe, plus a release marker.
fn build_fake_jdk_tar_gz(release: &str, version: &str, vendor: &str) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let add = |builder: &mut tar::Builder<Vec<u8>>, path: &str, body: &[u8], mode: u32| {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(mode);
        header.set_mtime(0);
        header.set_cksum();
        builder.append_data(&mut header, path, body).unwrap();
    };
    let script = format!(
        "#!/bin/sh\necho '  java.version = {version}' >&2\necho '  java.vendor = {vendor}' >&2\nexit 0\n"
    );
    add(
        &mut builder,
        &format!("{release}/release"),
        b"JAVA_VERSION=21\n",
        0o644,
    );
    add(
        &mut builder,
        &format!("{release}/bin/java"),
        script.as_bytes(),
        0o755,
    );
    // Incompressible filler: the download must span several 64 KiB chunks
    // so a mid-stream cancel has a window to land. xorshift64 output —
    // full-period, no structure for deflate to exploit.
    let filler: Vec<u8> = {
        let mut state: u64 = 0x1234_5678_9ABC_DEF0;
        (0..2 * 1024 * 1024u32)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 56) as u8
            })
            .collect()
    };
    add(
        &mut builder,
        &format!("{release}/lib/filler.bin"),
        &filler,
        0o644,
    );
    let tar_bytes = builder.into_inner().unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar_bytes).unwrap();
    encoder.finish().unwrap()
}

/// Serve the Adoptium shape (asset list + checksum link + package file)
/// for a prebuilt archive. The published checksum is always the real one
/// unless the test wants otherwise.
fn spawn_adoptium(
    archive: Vec<u8>,
    release: &str,
    published_sha: Option<String>,
    drip_ms: u64,
) -> (MockHttp, String) {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&archive);
    let real_sha: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let sha = published_sha.unwrap_or_else(|| real_sha.clone());
    let archive_for_handler = archive.clone();
    let release = release.to_owned();
    let base: Arc<std::sync::OnceLock<String>> = Arc::new(std::sync::OnceLock::new());
    let base_for_handler = Arc::clone(&base);
    let mock = MockHttp::spawn(move |path| {
        let base = base_for_handler.get().cloned().unwrap_or_default();
        if path.starts_with("/assets/latest/21") {
            MockHttpResponse::json(
                serde_json::to_vec(&json!([{
                    "release_name": release,
                    "binary": {
                        "package": {
                            "name": "OpenJDK21U-jdk_x64_linux_hotspot.tar.gz",
                            "link": format!("{base}/files/jdk.tar.gz"),
                            "size": archive_for_handler.len(),
                            "sha256link": format!("{base}/files/jdk.tar.gz.sha256"),
                        }
                    }
                }]))
                .unwrap(),
            )
        } else if path.contains(".sha256") {
            MockHttpResponse::json(format!("{sha}  OpenJDK21U-jdk_x64_linux_hotspot.tar.gz\n"))
        } else if path.contains("/files/jdk.tar.gz") {
            let response = MockHttpResponse::bytes(archive_for_handler.clone());
            if drip_ms > 0 {
                response.dripping(drip_ms)
            } else {
                response
            }
        } else {
            MockHttpResponse::not_found()
        }
    });
    let _ = base.set(mock.url.clone());
    (mock, real_sha)
}

fn unique_endpoint(tag: &str) -> zamin_ipc::Endpoint {
    zamin_ipc::Endpoint::unique_for_test(tag)
}

async fn wait_job(client: &mut common::Client, job_id: &str) -> (String, Option<Value>) {
    MockHttp::wait_job(client, job_id, Duration::from_secs(60)).await
}

#[tokio::test]
#[cfg(unix)]
async fn java_install_installs_inspects_and_lists() {
    let archive = build_fake_jdk_tar_gz("jdk-21-test+1", "21.0.99", "Test Temurin");
    let (adoptium, _sha) = spawn_adoptium(archive, "jdk-21-test+1", None, 0);
    let data_dir = scoped_dir("java-install");
    let endpoint = unique_endpoint("java-install");
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--adoptium-url", &adoptium.url]);
    let mut client = connect_daemon(&endpoint).await;

    // Before: the managed dir is empty; java.list still answers (with
    // whatever the machine itself has on PATH — possibly nothing).
    let before = client
        .request(methods::JAVA_LIST, json!({}))
        .await
        .expect("java.list");
    assert!(before["runtimes"].is_array());

    // Install Java 21.
    let result = client
        .request(
            methods::JAVA_INSTALL,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "majorVersion": 21}),
        )
        .await
        .expect("java.install");
    assert_eq!(result["kind"], "java.install");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, error) = wait_job(&mut client, &job_id).await;
    assert_eq!(state, "succeeded", "{error:?}");

    // The runtime is listed as managed and honestly inspected.
    let after = client
        .request(methods::JAVA_LIST, json!({}))
        .await
        .expect("java.list");
    let managed: Vec<&Value> = after["runtimes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["managed"] == true)
        .collect();
    assert_eq!(managed.len(), 1, "{:?}", after["runtimes"]);
    assert_eq!(managed[0]["major"], 21);
    assert_eq!(managed[0]["versionString"], "21.0.99");
    assert_eq!(managed[0]["vendor"], "Test Temurin");
    assert!(
        data_dir.join("java/jdk-21-test+1/bin/java").is_file(),
        "the extracted layout holds"
    );

    // No download artifact survives the success.
    let leftovers: Vec<String> = std::fs::read_dir(data_dir.join("java"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".part") || n.contains(".jdk-download"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    // Reinstall is idempotent and still answers.
    let result = client
        .request(
            methods::JAVA_INSTALL,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "majorVersion": 21}),
        )
        .await
        .expect("java.install again");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, error) = wait_job(&mut client, &job_id).await;
    assert_eq!(state, "succeeded", "{error:?}");
}

#[tokio::test]
async fn java_install_cancel_mid_download_leaves_nothing() {
    let archive = build_fake_jdk_tar_gz("jdk-21-test+1", "21.0.99", "Test Temurin");
    let (adoptium, _sha) = spawn_adoptium(archive, "jdk-21-test+1", None, 40);
    let data_dir = scoped_dir("java-cancel");
    let endpoint = unique_endpoint("java-cancel");
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--adoptium-url", &adoptium.url]);
    let mut client = connect_daemon(&endpoint).await;

    let result = client
        .request(
            methods::JAVA_INSTALL,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "majorVersion": 21}),
        )
        .await
        .expect("java.install");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    tokio::time::sleep(Duration::from_millis(300)).await;
    client
        .request(
            methods::JOBS_CANCEL,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "jobId": job_id}),
        )
        .await
        .expect("jobs.cancel");
    let (state, _) = wait_job(&mut client, &job_id).await;
    assert_eq!(state, "cancelled");
    assert!(!data_dir.join("java/jdk-21-test+1").exists());
}

#[tokio::test]
async fn java_install_bad_checksum_fails_typed() {
    let archive = build_fake_jdk_tar_gz("jdk-21-test+1", "21.0.99", "Test Temurin");
    let lie = "f".repeat(64);
    let (adoptium, _real) = spawn_adoptium(archive, "jdk-21-test+1", Some(lie), 0);
    let data_dir = scoped_dir("java-checksum");
    let endpoint = unique_endpoint("java-checksum");
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--adoptium-url", &adoptium.url]);
    let mut client = connect_daemon(&endpoint).await;

    let result = client
        .request(
            methods::JAVA_INSTALL,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "majorVersion": 21}),
        )
        .await
        .expect("java.install accepted");
    let job_id = result["job"]["jobId"].as_str().unwrap().to_owned();
    let (state, error) = wait_job(&mut client, &job_id).await;
    assert_eq!(state, "failed");
    assert_eq!(error.expect("typed error")["code"], "CHECKSUM_MISMATCH");
    assert!(!data_dir.join("java/jdk-21-test+1").exists());
}

#[tokio::test]
async fn java_install_unreachable_api_is_typed() {
    // Bind, learn the port, drop: nothing listens there anymore.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let data_dir = scoped_dir("java-dead");
    let endpoint = unique_endpoint("java-dead");
    let _daemon = spawn_daemon_with(&data_dir, &endpoint, &["--adoptium-url", &dead]);
    let mut client = connect_daemon(&endpoint).await;

    let error = client
        .request(
            methods::JAVA_INSTALL,
            json!({"requestId": uuid::Uuid::now_v7().to_string(), "majorVersion": 21}),
        )
        .await
        .expect_err("an unreachable API is a typed rejection, before any job");
    assert_eq!(error["code"], "CATALOG_UNAVAILABLE", "{error}");
}
