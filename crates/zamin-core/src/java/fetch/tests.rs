//! JDK fetch tests: Adoptium client against an in-process mock, plus
//! extraction safety for both archive formats. The zip path is tested
//! here on every platform by building archives with the same crate the
//! extractor reads; the tar path is the one CI exercises on both lanes
//! with real fixtures. The inspect step runs a real executable — the
//! fake-`java` fixture is a shell script, so those assertions are
//! unix-only, mirroring the honest platform story.

use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::{validate_release_name, AdoptiumClient};

#[cfg(unix)]
use super::{install_jdk, JdkAsset};
use crate::error::CoreError;
use crate::server::registry::tempdir;

// --- fixtures --------------------------------------------------------------

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// A minimal JDK archive layout: `jdk-<release>/bin/java` (+ a release
/// marker). The `java` fixture is a shell script that answers the
/// inspection probe (unix-only).
#[cfg(unix)]
fn fake_java_script() -> &'static str {
    "#!/bin/sh\necho '  java.version = 21.0.99' >&2\necho '  java.vendor = Test Temurin' >&2\nexit 0\n"
}

fn build_tar(top: &str, java_body: &str) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let add = |builder: &mut tar::Builder<Vec<u8>>, path: &str, body: &[u8], mode: u32| {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        builder.append_data(&mut header, path, body).unwrap();
    };
    add(
        &mut builder,
        &format!("{top}/release"),
        b"JAVA_VERSION=21\n",
        0o644,
    );
    // The platform's own exe name, exactly as build_zip does: the layout
    // contract is bin/<java_exe_name()> on every OS (a real Adoptium
    // tar.gz only ships on Linux, but the extractor's rule is one rule).
    add(
        &mut builder,
        &format!("{top}/bin/{}", crate::platform::java_exe_name()),
        java_body.as_bytes(),
        0o755,
    );
    builder.into_inner().unwrap()
}

fn build_tar_gz_gzipped(top: &str, java_body: &str) -> Vec<u8> {
    let tar_bytes = build_tar(top, java_body);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar_bytes).unwrap();
    encoder.finish().unwrap()
}

fn build_zip(top: &str, java_body: &str, exe_name: &str) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(format!("{top}/release"), options).unwrap();
        zip.write_all(b"JAVA_VERSION=21\n").unwrap();
        // The extractor looks for bin/<java_exe_name>; name the member
        // accordingly so the layout assertion holds on every platform.
        zip.start_file(format!("{top}/bin/{exe_name}"), options)
            .unwrap();
        zip.write_all(java_body.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    buffer.into_inner()
}

#[cfg(unix)]
fn asset(url: String, package: &str, sha: String) -> JdkAsset {
    JdkAsset {
        release_name: "jdk-21-test+1".to_owned(),
        package_name: package.to_owned(),
        url,
        sha256: sha,
        size: None,
    }
}

// --- release-name validation -----------------------------------------------

#[test]
fn release_names_are_validated() {
    assert!(validate_release_name("jdk-21.0.12.1+1").is_ok());
    for evil in ["../escape", "a/b", "a\\b", ".hidden", "", "x\0y"] {
        assert!(
            validate_release_name(evil).is_err(),
            "{evil:?} must be refused"
        );
    }
}

// --- Adoptium client -------------------------------------------------------

#[test]
fn latest_jdk_reads_the_api_and_checksum() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let request = String::from_utf8_lossy(&buf);
            let body = if request.contains("/assets/latest/21") {
                format!(
                    r#"[{{"release_name":"jdk-21-test+1","binary":{{"package":{{"name":"OpenJDK21.tar.gz","link":"http://{addr}/files/jdk.tar.gz","size":123,"sha256link":"http://{addr}/files/jdk.tar.gz.sha256"}}}}}}]"#
                )
            } else {
                format!("{SHA}  OpenJDK21.tar.gz\n")
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        }
    });
    let client = AdoptiumClient::new(&format!("http://{addr}"));
    let jdk = client.latest_jdk(21).expect("latest_jdk");
    assert_eq!(jdk.release_name, "jdk-21-test+1");
    assert_eq!(jdk.url, format!("http://{addr}/files/jdk.tar.gz"));
    assert_eq!(jdk.sha256, SHA);
    assert_eq!(jdk.size, Some(123));
}

// --- extraction safety -------------------------------------------------------

fn write_archive(
    name: &str,
    bytes: Vec<u8>,
    kind: &str,
) -> (tempdir::TempDirGuard, std::path::PathBuf) {
    // The extractor branches on the file name; the fixture must carry it.
    let guard = tempdir::scoped(name);
    let path = guard.path.clone().join(format!("archive.{kind}"));
    std::fs::write(&path, bytes).unwrap();
    (guard, path)
}

/// Direct extraction (the download steps are covered by the e2e suite).
fn extract_only(archive: &Path, managed: &Path, cancel: &Arc<AtomicBool>) -> std::path::PathBuf {
    let noop: Arc<dyn Fn(super::InstallProgress) + Send + Sync> = Arc::new(|_| {});
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.ends_with(".zip") {
        super::extract_zip(archive, managed, cancel, &noop).unwrap()
    } else {
        super::extract_tar_gz(archive, managed, cancel, &noop).unwrap()
    }
}

#[test]
fn tar_gz_extracts_and_the_layout_holds() {
    let (_guard, archive) = write_archive(
        "jdk-tar-happy",
        build_tar_gz_gzipped("jdk-21-test+1", "irrelevant"),
        "tar.gz",
    );
    let managed = tempdir::scoped("jdk-tar-happy-root");
    let cancel = Arc::new(AtomicBool::new(false));
    let runtime = extract_only(&archive, &managed.path, &cancel);
    assert_eq!(runtime, managed.path.join("jdk-21-test+1"));
    assert!(runtime.join("release").is_file());
    assert!(runtime
        .join("bin")
        .join(crate::platform::java_exe_name())
        .is_file());
}

#[test]
fn tar_gz_dotdot_is_refused() {
    // The tar crate refuses to WRITE dangerous names; the header bytes go
    // in by hand, exactly as a hostile producer would fill them.
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(2);
    header.set_mode(0o644);
    header.set_mtime(0);
    {
        let gnu = header.as_gnu_mut().unwrap();
        gnu.name = [0u8; 100];
        gnu.name[.."jdk-x/../../escaped.txt".len()].copy_from_slice(b"jdk-x/../../escaped.txt");
    }
    header.set_cksum();
    builder.append(&header, &b"no"[..]).unwrap();
    let bytes = builder.into_inner().unwrap();
    let (_guard, archive) = write_archive("jdk-tar-dotdot", bytes, "tar.gz");
    let managed = tempdir::scoped("jdk-tar-dotdot-root");
    let cancel = Arc::new(AtomicBool::new(false));
    let noop: Arc<dyn Fn(super::InstallProgress) + Send + Sync> = Arc::new(|_| {});
    let result = super::extract_tar_gz(&archive, &managed.path, &cancel, &noop);
    assert!(matches!(result, Err(CoreError::ArchiveUnsafeEntry { .. })));
    assert!(
        !managed.path.parent().unwrap().join("escaped.txt").exists(),
        "the escaped file must not land anywhere"
    );
}

#[test]
fn tar_gz_link_entries_are_refused() {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_cksum();
    builder
        .append_link(&mut header, "jdk-x/bin/evil", "/etc/passwd")
        .unwrap();
    let bytes = builder.into_inner().unwrap();
    let (_guard, archive) = write_archive("jdk-tar-link", bytes, "tar.gz");
    let managed = tempdir::scoped("jdk-tar-link-root");
    let cancel = Arc::new(AtomicBool::new(false));
    let noop: Arc<dyn Fn(super::InstallProgress) + Send + Sync> = Arc::new(|_| {});
    let result = super::extract_tar_gz(&archive, &managed.path, &cancel, &noop);
    assert!(matches!(result, Err(CoreError::ArchiveUnsafeEntry { .. })));
}

#[test]
fn tar_gz_split_tops_are_refused() {
    let mut builder = tar::Builder::new(Vec::new());
    for path in ["jdk-a/release", "jdk-b/release"] {
        let mut header = tar::Header::new_gnu();
        header.set_size(2);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, &b"no"[..]).unwrap();
    }
    let bytes = builder.into_inner().unwrap();
    let (_guard, archive) = write_archive("jdk-tar-split", bytes, "tar.gz");
    let managed = tempdir::scoped("jdk-tar-split-root");
    let cancel = Arc::new(AtomicBool::new(false));
    let noop: Arc<dyn Fn(super::InstallProgress) + Send + Sync> = Arc::new(|_| {});
    let result = super::extract_tar_gz(&archive, &managed.path, &cancel, &noop);
    assert!(matches!(result, Err(CoreError::ArchiveUnsafeEntry { .. })));
}

// --- extraction safety (zip, cross-platform) --------------------------------

#[test]
fn zip_extracts_with_the_expected_layout() {
    let exe = crate::platform::java_exe_name();
    let bytes = build_zip("jdk-21-test+1", "irrelevant", exe);
    let (_guard, archive) = write_archive("jdk-zip-happy", bytes, "zip");
    let managed = tempdir::scoped("jdk-zip-happy-root");
    let cancel = Arc::new(AtomicBool::new(false));
    let runtime = extract_only(&archive, &managed.path, &cancel);
    assert_eq!(runtime, managed.path.join("jdk-21-test+1"));
    let listing: Vec<String> = walk(&managed.path);
    assert!(
        listing.iter().any(|e| e.ends_with(&format!("bin/{exe}"))),
        "expected bin/{exe} in {listing:?}"
    );
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() {
            for sub in walk(&entry.path()) {
                out.push(format!("{name}/{}", sub));
            }
        } else {
            out.push(name);
        }
    }
    out
}

#[test]
fn zip_dotdot_is_refused() {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("../escaped.txt", options).unwrap();
        zip.write_all(b"no").unwrap();
        zip.finish().unwrap();
    }
    let (_guard, archive) = write_archive("jdk-zip-dotdot", buffer.into_inner(), "zip");
    let managed = tempdir::scoped("jdk-zip-dotdot-root");
    let cancel = Arc::new(AtomicBool::new(false));
    let noop: Arc<dyn Fn(super::InstallProgress) + Send + Sync> = Arc::new(|_| {});
    let result = super::extract_zip(&archive, &managed.path, &cancel, &noop);
    assert!(matches!(result, Err(CoreError::ArchiveUnsafeEntry { .. })));
}

// --- the full install (unix: the fake java runs) -----------------------------

#[cfg(unix)]
fn serve_jdk_files(bytes: Vec<u8>, sha: String) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let request = String::from_utf8_lossy(&buf);
            let (ctype, body) = if request.contains(".sha256") {
                (
                    "text/plain",
                    format!("{sha}  OpenJDK21.tar.gz\n").into_bytes(),
                )
            } else {
                ("application/octet-stream", bytes.clone())
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://{addr}")
}

#[cfg(unix)]
#[test]
fn full_install_downloads_extracts_and_inspects() {
    let bytes = build_tar_gz_gzipped("jdk-21-test+1", fake_java_script());
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let base = serve_jdk_files(bytes, sha.clone());

    let managed = tempdir::scoped("jdk-install-root");
    let jdk = asset(format!("{base}/files/jdk.tar.gz"), "OpenJDK21.tar.gz", sha);
    let outcome = install_jdk(
        &managed.path,
        &jdk,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
        None,
    )
    .expect("install");
    assert!(!outcome.already_installed);
    assert_eq!(outcome.major, 21);
    assert_eq!(outcome.vendor, "Test Temurin");
    assert_eq!(
        outcome.version_string, "21.0.99",
        "inspection reads the runtime, not the directory name"
    );
    assert!(outcome.java_path.is_file());
    assert!(outcome.runtime_dir.starts_with(&managed.path));

    // Discovery now finds it among managed candidates.
    let candidates = crate::java::managed_candidates(&managed.path);
    assert!(candidates.contains(&outcome.java_path));

    // Second install of the same release: idempotent, still inspected.
    let again = install_jdk(
        &managed.path,
        &jdk,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
        None,
    )
    .expect("reinstall");
    assert!(again.already_installed);
    assert_eq!(again.java_path, outcome.java_path);
}

#[cfg(unix)]
#[test]
fn checksum_mismatch_refuses_to_extract() {
    let bytes = build_tar_gz_gzipped("jdk-21-test+1", fake_java_script());
    // Serve real bytes, publish a lying checksum: the downloader must
    // reject before any extraction happens.
    let base = serve_jdk_files(bytes, "f".repeat(64));

    let managed = tempdir::scoped("jdk-mismatch-root");
    let jdk = asset(
        format!("{base}/files/jdk.tar.gz"),
        "OpenJDK21.tar.gz",
        "f".repeat(64),
    );
    let result = install_jdk(
        &managed.path,
        &jdk,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
        None,
    );
    assert!(matches!(result, Err(CoreError::ChecksumMismatch { .. })));
    assert!(
        !managed.path.join("jdk-21-test+1").exists(),
        "a failed install extracts nothing"
    );
}

#[cfg(unix)]
#[test]
fn a_cached_jdk_installs_offline() {
    // The cache story end to end: the first install (network up) stores
    // the archive; the server then DIES; the second install of the same
    // URL still works — the cache's validated hit answers before any
    // network is touched.
    use std::sync::atomic::{AtomicBool, Ordering};

    let bytes = build_tar_gz_gzipped("jdk-21-test+1", fake_java_script());
    let sha = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };

    // A controllable one-file server: flipping `down` closes the socket.
    let down = Arc::new(AtomicBool::new(false));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let down_thread = Arc::downgrade(&down);
    listener.set_nonblocking(true).unwrap();
    let sha_thread = sha.clone();
    let server = std::thread::spawn(move || {
        let sha = sha_thread;
        loop {
            if down_thread.upgrade().map(|d| d.load(Ordering::Relaxed)) == Some(true) {
                return;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 4096];
                    let _ = std::io::Read::read(&mut stream, &mut buf);
                    let request = String::from_utf8_lossy(&buf);
                    let (ctype, body) = if request.contains(".sha256") {
                        (
                            "text/plain",
                            format!("{sha}  OpenJDK21.tar.gz\n").into_bytes(),
                        )
                    } else {
                        ("application/gzip", bytes.clone())
                    };
                    let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&body);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => return,
            }
        }
    });
    let base = format!("http://{addr}");

    let managed = tempdir::scoped("jdk-offline-root");
    let cache_dir = tempdir::scoped("jdk-offline-cache");
    let cache = crate::cache::Cache::new(cache_dir.path.to_path_buf());
    let jdk = asset(format!("{base}/files/jdk.tar.gz"), "OpenJDK21.tar.gz", sha);

    let first = install_jdk(
        &managed.path,
        &jdk,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
        Some(&cache),
    )
    .expect("online install");
    assert!(!first.already_installed);
    assert!(cache.lookup(&jdk.url).is_some(), "the archive is cached");

    // The network dies; the extracted runtime goes too (a re-install).
    down.store(true, Ordering::Relaxed);
    server.join().unwrap();
    std::fs::remove_dir_all(&first.runtime_dir).unwrap();

    let second = install_jdk(
        &managed.path,
        &jdk,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
        Some(&cache),
    )
    .expect("offline install from the cache");
    assert!(!second.already_installed);
    assert!(second.java_path.is_file());
}
