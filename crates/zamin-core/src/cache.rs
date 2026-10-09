//! The runtime cache: immutable downloadable assets — server jars, JDK
//! archives — live once under `<data>/cache/<sha256(url)>/`, validated
//! by their digest on EVERY hit. The install flow is: check the cache →
//! validate the artifact → use it if sound → download only when
//! necessary → store → and every later install of the same asset works
//! offline.
//!
//! The cache is honest, not a URL memory: a hit re-hashes the file and
//! compares against the meta record — a corrupted or truncated artifact
//! is a miss (and its file is dropped), never a plausible-looking lie.
//! Blocking on purpose: callers already run inside `spawn_blocking`.

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

use crate::error::CoreError;
use crate::software::{DownloadOptions, DownloadOutcome, Verified};

const META_FILE: &str = "meta.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CacheMeta {
    url: String,
    size: u64,
    digest: String,
    algorithm: String,
    file: String,
}

/// One validated cached artifact.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedArtifact {
    pub path: PathBuf,
    pub size: u64,
    pub digest: String,
}

pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: PathBuf) -> Cache {
        Cache { root }
    }

    /// The entry key: sha256 of the URL. The URL is the identity of the
    /// immutable asset; two servers installing the same jar share one
    /// entry.
    fn key(url: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn entry_dir(&self, url: &str) -> PathBuf {
        self.root.join(Self::key(url))
    }

    fn read_meta(entry: &Path) -> Option<CacheMeta> {
        let bytes = std::fs::read(entry.join(META_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Hash a file with the meta record's algorithm. A hash we cannot
    /// compute is a validation failure (the caller drops the artifact).
    fn hash_file(path: &Path, algorithm: &str) -> Result<String, CoreError> {
        let file = File::open(path).map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut reader = BufReader::new(file);
        let mut chunk = [0u8; 64 * 1024];
        match algorithm {
            "sha512" => {
                let mut hasher = Sha512::new();
                loop {
                    let read = reader.read(&mut chunk).map_err(|source| CoreError::Io {
                        path: path.to_path_buf(),
                        source,
                    })?;
                    if read == 0 {
                        break;
                    }
                    hasher.update(&chunk[..read]);
                }
                Ok(hasher
                    .finalize()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect())
            }
            _ => {
                let mut hasher = Sha256::new();
                loop {
                    let read = reader.read(&mut chunk).map_err(|source| CoreError::Io {
                        path: path.to_path_buf(),
                        source,
                    })?;
                    if read == 0 {
                        break;
                    }
                    hasher.update(&chunk[..read]);
                }
                Ok(hasher
                    .finalize()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect())
            }
        }
    }

    /// A validated hit, or None. Validation: the meta record exists, the
    /// artifact's size matches, and its re-hashed digest matches. An
    /// unsound artifact is deleted on sight (a miss, and the entry
    /// cannot poison the next caller).
    pub fn lookup(&self, url: &str) -> Option<CachedArtifact> {
        let entry = self.entry_dir(url);
        let meta = Self::read_meta(&entry)?;
        let path = entry.join(&meta.file);
        let Ok(size) = std::fs::metadata(&path) else {
            return None;
        };
        if size.len() != meta.size {
            let _ = std::fs::remove_file(&path);
            return None;
        }
        let actual = Self::hash_file(&path, &meta.algorithm).ok()?;
        if actual != meta.digest {
            let _ = std::fs::remove_file(&path);
            return None;
        }
        Some(CachedArtifact {
            path,
            size: meta.size,
            digest: actual,
        })
    }

    /// Publish a downloaded staging file into the cache. The digest is
    /// the downloader's verified one (the download path always hashes
    /// the stream, even when no checksum was published upstream).
    fn store(
        &self,
        url: &str,
        staging: &Path,
        digest: &str,
        algorithm: &str,
        file_name: &str,
    ) -> Result<CachedArtifact, CoreError> {
        let entry = self.entry_dir(url);
        std::fs::create_dir_all(&entry).map_err(|source| CoreError::Io {
            path: entry.clone(),
            source,
        })?;
        let size = std::fs::metadata(staging)
            .map_err(|source| CoreError::Io {
                path: staging.to_path_buf(),
                source,
            })?
            .len();
        let dest = entry.join(file_name);
        std::fs::rename(staging, &dest).map_err(|source| CoreError::Io {
            path: dest.clone(),
            source,
        })?;
        let meta = CacheMeta {
            url: url.to_owned(),
            size,
            digest: digest.to_owned(),
            algorithm: algorithm.to_owned(),
            file: file_name.to_owned(),
        };
        let meta_bytes = serde_json::to_vec(&meta).map_err(|source| CoreError::Io {
            path: entry.join(META_FILE),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, source.to_string()),
        })?;
        let mut meta_file =
            File::create(entry.join(META_FILE)).map_err(|source| CoreError::Io {
                path: entry.join(META_FILE),
                source,
            })?;
        meta_file
            .write_all(&meta_bytes)
            .map_err(|source| CoreError::Io {
                path: entry.join(META_FILE),
                source,
            })?;
        meta_file.sync_all().map_err(|source| CoreError::Io {
            path: entry.join(META_FILE),
            source,
        })?;
        Ok(CachedArtifact {
            path: dest,
            size,
            digest: digest.to_owned(),
        })
    }

    /// The full flow: check → validate → use; miss → download into the
    /// cache's staging area (the caller's published checksum and
    /// progress/cancel ride along) → publish → return. Offline installs
    /// are the hit path; nothing about the miss path changes the
    /// downloader's own verification discipline.
    pub fn fetch(
        &self,
        url: &str,
        expected: Option<Verified<'_>>,
        file_name: &str,
        options: &DownloadOptions,
    ) -> Result<CachedArtifact, CoreError> {
        if let Some(hit) = self.lookup(url) {
            return Ok(hit);
        }
        let entry = self.entry_dir(url);
        std::fs::create_dir_all(&entry).map_err(|source| CoreError::Io {
            path: entry.clone(),
            source,
        })?;
        let staging = entry.join(format!(".staging-{}.part", std::process::id()));
        let outcome = crate::software::download_to_staging(url, &staging, expected, options);
        match outcome {
            Ok(downloaded) => {
                let algorithm = if matches!(expected, Some(Verified::Sha512(_))) {
                    "sha512"
                } else {
                    "sha256"
                };
                match self.store(url, &staging, &downloaded.digest, algorithm, file_name) {
                    Ok(artifact) => Ok(artifact),
                    Err(store_error) => {
                        let _ = std::fs::remove_file(&staging);
                        Err(store_error)
                    }
                }
            }
            Err(error) => {
                let _ = std::fs::remove_file(&staging);
                Err(error)
            }
        }
    }
}

/// Install a cached artifact into a destination with the downloader's
/// atomic discipline: stage → fsync → rename, refusing an existing
/// target unless the caller opts into replacement. The cache itself is
/// never mutated by an install.
pub fn install_from_cache(
    artifact: &Path,
    artifact_digest: &str,
    dir: &Path,
    file_name: &str,
    options: &DownloadOptions,
) -> Result<DownloadOutcome, CoreError> {
    std::fs::create_dir_all(dir).map_err(|source| CoreError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let dest = dir.join(file_name);
    if dest.exists() && !options.replace {
        return Err(CoreError::Io {
            path: dest,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "install target already exists",
            ),
        });
    }
    let staging = dir.join(format!(".zamin-staging-{}.part", std::process::id()));
    let result = (|| {
        std::fs::copy(artifact, &staging).map_err(|source| CoreError::Io {
            path: staging.to_path_buf(),
            source,
        })?;
        let file = File::open(&staging).map_err(|source| CoreError::Io {
            path: staging.to_path_buf(),
            source,
        })?;
        file.sync_all().map_err(|source| CoreError::Io {
            path: staging.to_path_buf(),
            source,
        })?;
        drop(file);
        std::fs::rename(&staging, &dest).map_err(|source| CoreError::Io {
            path: dest.clone(),
            source,
        })?;
        let size = std::fs::metadata(&dest)
            .map_err(|source| CoreError::Io {
                path: dest.clone(),
                source,
            })?
            .len();
        Ok(DownloadOutcome {
            path: dest,
            size,
            // The artifact's digest is the cache's verified record.
            digest: artifact_digest.to_owned(),
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::AtomicBool;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zamin-cache-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_staging(path: &Path, bytes: &[u8]) {
        let mut file = File::create(path).unwrap();
        file.write_all(bytes).unwrap();
    }

    fn digest_of(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn store_then_hit_round_trips() {
        let root = temp_root("roundtrip");
        let cache = Cache::new(root.clone());
        let url = "https://example.test/paper-1.21.jar";
        let bytes = b"jar bytes";
        let staging = root.join("staging.part");
        write_staging(&staging, bytes);
        let artifact = cache
            .store(url, &staging, &digest_of(bytes), "sha256", "server.jar")
            .unwrap();
        let hit = cache.lookup(url).expect("the hit");
        assert_eq!(hit.path, artifact.path);
        assert_eq!(hit.digest, digest_of(bytes));
        assert_eq!(hit.size, bytes.len() as u64);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_corrupted_artifact_is_a_miss_and_is_dropped() {
        let root = temp_root("corrupt");
        let cache = Cache::new(root.clone());
        let url = "https://example.test/paper-1.21.jar";
        let bytes = b"jar bytes";
        let staging = root.join("staging.part");
        write_staging(&staging, bytes);
        cache
            .store(url, &staging, &digest_of(bytes), "sha256", "server.jar")
            .unwrap();
        // Corrupt the artifact BEHIND the meta record's back.
        let artifact_path = cache.lookup(url).unwrap().path;
        write_staging(&artifact_path, b"tampered bytes!!");
        assert!(cache.lookup(url).is_none(), "a lie is not a hit");
        assert!(!artifact_path.exists(), "the unsound artifact is dropped");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_truncated_artifact_is_a_miss() {
        let root = temp_root("truncate");
        let cache = Cache::new(root.clone());
        let url = "https://example.test/jdk.tar.gz";
        let bytes = b"archive bytes";
        let staging = root.join("staging.part");
        write_staging(&staging, bytes);
        cache
            .store(url, &staging, &digest_of(bytes), "sha256", "jdk.tar.gz")
            .unwrap();
        let artifact_path = cache.lookup(url).unwrap().path;
        write_staging(&artifact_path, b"jar");
        assert!(cache.lookup(url).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn install_from_cache_refuses_an_existing_target() {
        let root = temp_root("install");
        let artifact = root.join("artifact.jar");
        write_staging(&artifact, b"jar");
        let dest_dir = root.join("instance");
        std::fs::create_dir_all(&dest_dir).unwrap();
        std::fs::write(dest_dir.join("server.jar"), b"old").unwrap();
        let options = DownloadOptions {
            cancel: std::sync::Arc::new(AtomicBool::new(false)),
            progress: None,
            replace: false,
        };
        assert!(install_from_cache(
            &artifact,
            &digest_of(b"jar"),
            &dest_dir,
            "server.jar",
            &options
        )
        .is_err());
        // The old bytes survive a refused install.
        assert_eq!(std::fs::read(dest_dir.join("server.jar")).unwrap(), b"old");
        // And the replace lane installs over them.
        let options = DownloadOptions {
            cancel: std::sync::Arc::new(AtomicBool::new(false)),
            progress: None,
            replace: true,
        };
        install_from_cache(
            &artifact,
            &digest_of(b"jar"),
            &dest_dir,
            "server.jar",
            &options,
        )
        .unwrap();
        assert_eq!(std::fs::read(dest_dir.join("server.jar")).unwrap(), b"jar");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn distinct_urls_never_share_an_entry() {
        let root = temp_root("distinct");
        let cache = Cache::new(root.clone());
        let bytes_a: &[u8] = b"alpha";
        let bytes_b: &[u8] = b"beta";
        for (url, bytes, name) in [
            ("https://example.test/a.jar", bytes_a, "a.jar"),
            ("https://example.test/b.jar", bytes_b, "b.jar"),
        ] {
            let staging = root.join(format!("{name}.part"));
            write_staging(&staging, bytes);
            cache
                .store(url, &staging, &digest_of(bytes), "sha256", name)
                .unwrap();
        }
        assert_ne!(
            cache.lookup("https://example.test/a.jar").unwrap().digest,
            cache.lookup("https://example.test/b.jar").unwrap().digest
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
