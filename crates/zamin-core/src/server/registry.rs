//! The daemon-owned index of managed servers: `registry.json` in app data,
//! versioned, atomically written (ADR-0004, ADR-0007).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::fsops::atomic_write;
use crate::server::ServerId;

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    pub server_id: ServerId,
    pub display_name: String,
    /// Canonical server root. Known only to the daemon (ADR-0004).
    pub root: PathBuf,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    servers: Vec<RegistryEntry>,
}

#[derive(Debug, Default)]
pub struct Registry {
    path: PathBuf,
    entries: BTreeMap<ServerId, RegistryEntry>,
}

impl Registry {
    /// Load, or start empty when absent. Unknown fields are preserved out of
    /// scope here — the registry round-trips only known data; a corrupt or
    /// wrong-version file is a loud error, never a silent reset.
    pub fn load(path: impl Into<PathBuf>) -> Result<Registry, CoreError> {
        let path = path.into();
        if !path.exists() {
            return Ok(Registry {
                path,
                entries: BTreeMap::new(),
            });
        }
        let bytes = std::fs::read(&path).map_err(|source| CoreError::Io {
            path: path.clone(),
            source,
        })?;
        let file: RegistryFile =
            serde_json::from_slice(&bytes).map_err(|e| CoreError::RegistryCorrupt {
                path: path.clone(),
                reason: e.to_string(),
            })?;
        if file.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(CoreError::SchemaVersion {
                path,
                found: file.schema_version,
                expected: REGISTRY_SCHEMA_VERSION,
            });
        }
        let entries = file
            .servers
            .into_iter()
            .map(|e| (e.server_id.clone(), e))
            .collect();
        Ok(Registry { path, entries })
    }

    pub fn save(&self) -> Result<(), CoreError> {
        let file = RegistryFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            servers: self.entries.values().cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(|e| CoreError::RegistryCorrupt {
            path: self.path.clone(),
            reason: e.to_string(),
        })?;
        atomic_write(&self.path, &bytes)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, id: &ServerId) -> Option<&RegistryEntry> {
        self.entries.get(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &RegistryEntry> {
        self.entries.values()
    }

    /// Register a server. `root` is canonicalized; both the id and the root
    /// must be unique — a collision is an error, never a merge.
    pub fn register(
        &mut self,
        server_id: ServerId,
        display_name: String,
        root: impl Into<PathBuf>,
    ) -> Result<&RegistryEntry, CoreError> {
        let root_arg: PathBuf = root.into();
        let root = std::fs::canonicalize(&root_arg).map_err(|source| CoreError::Io {
            path: root_arg.clone(),
            source,
        })?;
        if self.entries.contains_key(&server_id) {
            return Err(CoreError::ServerAlreadyRegistered {
                id: server_id.to_string(),
            });
        }
        if self.entries.values().any(|e| e.root == root) {
            return Err(CoreError::ServerRootAlreadyRegistered { path: root });
        }
        let entry = RegistryEntry {
            server_id: server_id.clone(),
            display_name,
            root,
            created_at_ms: now_ms(),
        };
        // Insert, persist, and roll back on a failed save: memory and
        // disk must not diverge (an entry visible here but absent after a
        // restart would be a silent lie).
        self.entries.insert(entry.server_id.clone(), entry);
        let result = self.save();
        if result.is_err() {
            self.entries.remove(&server_id);
            result?;
        }
        self.entries
            .get(&server_id)
            .ok_or_else(|| CoreError::ServerNotRegistered {
                id: server_id.to_string(),
            })
    }

    pub fn remove(&mut self, server_id: &ServerId) -> Result<(), CoreError> {
        if self.entries.remove(server_id).is_none() {
            return Err(CoreError::ServerNotRegistered {
                id: server_id.to_string(),
            });
        }
        self.save()
    }

    pub fn rename(&mut self, server_id: &ServerId, display_name: String) -> Result<(), CoreError> {
        let entry =
            self.entries
                .get_mut(server_id)
                .ok_or_else(|| CoreError::ServerNotRegistered {
                    id: server_id.to_string(),
                })?;
        entry.display_name = display_name;
        self.save()
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_registry(tag: &str) -> (std::path::PathBuf, tempdir::TempDirGuard) {
        let dir = tempdir::scoped(tag);
        (dir.path.join("registry.json"), dir)
    }

    #[test]
    fn round_trips_entries() {
        let (path, _guard) = temp_registry("roundtrip");
        let root = tempdir::scoped("roundtrip-root");
        let mut registry = Registry::load(&path).unwrap();
        assert_eq!(registry.all().count(), 0);

        registry
            .register(
                ServerId::parse("production").unwrap(),
                "Production".into(),
                &root.path,
            )
            .unwrap();
        drop(registry);

        let registry = Registry::load(&path).unwrap();
        assert_eq!(registry.all().count(), 1);
        let entry = registry
            .get(&ServerId::parse("production").unwrap())
            .unwrap();
        assert_eq!(entry.display_name, "Production");
        assert_eq!(entry.root, std::fs::canonicalize(&root.path).unwrap());
    }

    #[test]
    fn rejects_duplicate_id_and_duplicate_root() {
        let (_path, _guard) = temp_registry("dup");
        let root = tempdir::scoped("dup-root");
        let mut registry =
            Registry::load(tempdir::scoped("dup-file").path.join("registry.json")).unwrap();

        registry
            .register(ServerId::parse("a").unwrap(), "A".into(), &root.path)
            .unwrap();
        assert!(registry
            .register(ServerId::parse("a").unwrap(), "A again".into(), &root.path)
            .is_err());
        assert!(registry
            .register(ServerId::parse("b").unwrap(), "B".into(), &root.path)
            .is_err());
    }

    #[test]
    fn wrong_schema_version_is_a_loud_error() {
        let dir = tempdir::scoped("schema");
        let path = dir.path.join("registry.json");
        std::fs::write(&path, br#"{"schemaVersion": 99, "servers": []}"#).unwrap();
        assert!(matches!(
            Registry::load(&path),
            Err(CoreError::SchemaVersion { .. })
        ));
    }
}

/// Minimal scoped temp directories for tests; real mktemp semantics without
/// pulling a dependency for five lines.
#[cfg(test)]
pub(crate) mod tempdir {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    pub struct TempDirGuard {
        pub path: PathBuf,
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    pub fn scoped(tag: &str) -> TempDirGuard {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zamin-core-test-{}-{}-{}",
            tag,
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir creatable");
        TempDirGuard { path }
    }
}
