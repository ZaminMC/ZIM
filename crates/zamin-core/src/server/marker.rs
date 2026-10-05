//! The marker file binding a server root to its `serverId`
//! (`.zamin/server.json`): the rediscovery anchor when the registry entry
//! and the directory have to be reconciled (ADR-0004).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::fsops::atomic_write;
use crate::server::ServerId;

pub const MARKER_SCHEMA_VERSION: u32 = 1;
pub const MARKER_DIR: &str = ".zamin";
pub const MARKER_FILE: &str = ".zamin/server.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct MarkerFile {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "serverId")]
    server_id: ServerId,
}

/// Write the marker into a server root. Idempotent.
pub fn write_marker(root: &Path, server_id: &ServerId) -> Result<(), CoreError> {
    let marker = MarkerFile {
        schema_version: MARKER_SCHEMA_VERSION,
        server_id: server_id.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&marker).map_err(|e| CoreError::RegistryCorrupt {
        path: root.to_path_buf(),
        reason: e.to_string(),
    })?;
    atomic_write(&root.join(MARKER_FILE), &bytes)
}

/// `Some(id)` when a valid marker exists; `None` when absent. A corrupt or
/// wrong-version marker is an error — the root is managed-shaped but
/// unreadable, which must surface, not silently read as unmanaged.
pub fn read_marker(root: &Path) -> Result<Option<ServerId>, CoreError> {
    let path = root.join(MARKER_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|source| CoreError::Io {
        path: path.clone(),
        source,
    })?;
    let marker: MarkerFile =
        serde_json::from_slice(&bytes).map_err(|e| CoreError::RegistryCorrupt {
            path: path.clone(),
            reason: e.to_string(),
        })?;
    if marker.schema_version != MARKER_SCHEMA_VERSION {
        return Err(CoreError::SchemaVersion {
            path,
            found: marker.schema_version,
            expected: MARKER_SCHEMA_VERSION,
        });
    }
    Ok(Some(marker.server_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::registry::tempdir;

    #[test]
    fn marker_round_trips() {
        let dir = tempdir::scoped("marker");
        let id = ServerId::parse("production").unwrap();
        write_marker(&dir.path, &id).unwrap();
        assert_eq!(read_marker(&dir.path).unwrap(), Some(id));
    }

    #[test]
    fn absent_marker_is_none() {
        let dir = tempdir::scoped("marker-absent");
        assert_eq!(read_marker(&dir.path).unwrap(), None);
    }

    #[test]
    fn corrupt_marker_is_an_error() {
        let dir = tempdir::scoped("marker-bad");
        std::fs::create_dir_all(dir.path.join(MARKER_DIR)).unwrap();
        std::fs::write(dir.path.join(MARKER_FILE), b"not json at all").unwrap();
        assert!(read_marker(&dir.path).is_err());
    }
}
