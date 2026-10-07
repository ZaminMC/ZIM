//! The publication record — what actually went out last time — and the
//! false-positive reviews. Both live beside the server's other daemon
//! metadata (registry precedent), NEVER inside the server root: the
//! record describes the published tree, so it must not be able to
//! publish itself.
//!
//! Crash semantics (founder §42): writes go through the fsops atomic
//! write (staging temp + rename with retries), so a torn write leaves
//! the OLD record, never a half one. The record commits only after the
//! provider answers — a crash mid-upload leaves the previous record and
//! a stray package, which the next publish overwrites. Corruption on
//! load is a loud typed error, never a silent reset (ADR-0007): the
//! diff would otherwise lie about what was published.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use zamin_protocol::publish::{ReviewEntry, UploadReceipt};

use crate::error::CoreError;
use crate::fsops::atomic_write;

pub const PUBLICATION_SCHEMA_VERSION: u32 = 1;

/// One published file's identity: digest + size, keyed by its
/// root-relative path in the record's map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationFile {
    pub sha512: String,
    pub size: u64,
}

/// The durable state of the last publication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationRecord {
    pub schema_version: u32,
    pub published_at_ms: i64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<String>,
    pub provider_id: String,
    pub receipt: UploadReceipt,
    pub package_sha512: String,
    pub package_bytes: u64,
    pub files: BTreeMap<String, PublicationFile>,
}

/// Load the record. Absent file = never published (`None`); corrupt
/// file or unknown schema = typed error, never a silent "nothing
/// published" (that would make the next diff say "everything added").
pub fn load_publication(path: &Path) -> Result<Option<PublicationRecord>, CoreError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(CoreError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    let record: PublicationRecord =
        serde_json::from_slice(&bytes).map_err(|e| CoreError::PublishStateCorrupt {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    if record.schema_version != PUBLICATION_SCHEMA_VERSION {
        return Err(CoreError::PublishStateCorrupt {
            path: path.to_path_buf(),
            reason: format!(
                "schema version {}, expected {PUBLICATION_SCHEMA_VERSION}",
                record.schema_version
            ),
        });
    }
    Ok(Some(record))
}

/// Commit the record atomically: the rename either happens or it does
/// not; there is no half-publication on disk in either case.
pub fn save_publication(path: &Path, record: &PublicationRecord) -> Result<(), CoreError> {
    let mut bytes =
        serde_json::to_vec_pretty(record).map_err(|e| CoreError::PublishStateCorrupt {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

/// Load the review set (§46: false positives an operator has marked).
/// Absent file = nothing reviewed yet; the reviews file is small and
/// operator-authored, so corruption is a typed error like the record's.
pub fn load_reviews(path: &Path) -> Result<BTreeSet<ReviewEntry>, CoreError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => {
            return Err(CoreError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    serde_json::from_slice(&bytes).map_err(|e| CoreError::PublishStateCorrupt {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

pub fn save_reviews(path: &Path, reviews: &BTreeSet<ReviewEntry>) -> Result<(), CoreError> {
    let mut bytes =
        serde_json::to_vec_pretty(reviews).map_err(|e| CoreError::PublishStateCorrupt {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}
