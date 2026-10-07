//! The §40 provider interface — and the two honest built-ins.
//!
//! "Do not hardcode marketplace-specific behavior into the core" is a
//! founder rule taken literally: marketplaces arrive as new
//! implementations of `PublishProvider`, registered here; nothing in the
//! pipeline knows their names. The two built-ins are the ones that need
//! no network and no credential, so the whole publish pipeline is
//! exercisable end to end today (and in tests):
//!
//! - `archive` — the package is built and kept; no upload happens.
//! - `local-dir` — the package is copied into an operator-chosen
//!   folder with its receipt beside it (a watched/synced folder makes
//!   this a real distribution channel).
//!
//! Credential handling (§47) is part of the contract: a provider that
//! needs one declares it, and the credential arrives through the
//! environment channel at execute time — never through config, packages,
//! logs, or this interface's receipts.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use zamin_protocol::publish::{ProviderSettingInfo, UploadReceipt};

use crate::error::CoreError;
use crate::publish::package::PublishManifest;

/// The artifact a provider uploads.
pub struct ProviderPackageRef<'a> {
    pub path: &'a Path,
    pub sha512: &'a str,
    pub size_bytes: u64,
    pub manifest: &'a PublishManifest,
}

/// One publish destination. Implementations must be send + sync and
/// must never persist credentials anywhere.
pub trait PublishProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn needs_credential(&self) -> bool;
    /// The environment variable a credential rides in, when needed.
    fn credential_env_var(&self) -> Option<String>;
    /// The settings keys this provider understands, described for UIs.
    fn settings(&self) -> Vec<ProviderSettingInfo>;
    /// Validate non-secret settings. Unknown keys are refused: a typo in
    /// `outDir` must not silently mean "no destination".
    fn validate_settings(&self, settings: &BTreeMap<String, String>) -> Result<(), CoreError>;
    /// Hand the package over. Returns the provider's receipt. A failure
    /// is typed (`PublishUpload`) and carries no credential material.
    fn upload(
        &self,
        package: &ProviderPackageRef<'_>,
        settings: &BTreeMap<String, String>,
        credential: Option<&str>,
    ) -> Result<UploadReceipt, CoreError>;
}

pub fn all_providers() -> Vec<&'static dyn PublishProvider> {
    vec![&ArchiveProvider, &LocalDirProvider]
}

pub fn provider_by_id(id: &str) -> Option<&'static dyn PublishProvider> {
    all_providers().into_iter().find(|p| p.id() == id)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn invalid(reason: impl Into<String>) -> CoreError {
    CoreError::InvalidPublishConfig {
        reason: reason.into(),
    }
}

/// The no-upload destination: the package exists, the receipt says so.
struct ArchiveProvider;

impl PublishProvider for ArchiveProvider {
    fn id(&self) -> &'static str {
        "archive"
    }

    fn display_name(&self) -> &'static str {
        "Archive only (no upload)"
    }

    fn needs_credential(&self) -> bool {
        false
    }

    fn credential_env_var(&self) -> Option<String> {
        None
    }

    fn settings(&self) -> Vec<ProviderSettingInfo> {
        Vec::new()
    }

    fn validate_settings(&self, settings: &BTreeMap<String, String>) -> Result<(), CoreError> {
        if let Some(key) = settings.keys().next() {
            return Err(invalid(format!(
                "the archive provider takes no settings; found {:?}",
                key
            )));
        }
        Ok(())
    }

    fn upload(
        &self,
        package: &ProviderPackageRef<'_>,
        _settings: &BTreeMap<String, String>,
        _credential: Option<&str>,
    ) -> Result<UploadReceipt, CoreError> {
        Ok(UploadReceipt {
            provider_id: self.id().to_owned(),
            reference: format!("sha512:{}", &package.sha512[..12.min(package.sha512.len())]),
            detail: Some(
                "the package stayed in the daemon's publish directory; no upload was performed"
                    .to_owned(),
            ),
            at_ms: now_ms(),
        })
    }
}

/// The local-folder destination: the package plus its receipt land in an
/// operator-chosen absolute folder.
struct LocalDirProvider;

const OUT_DIR_KEY: &str = "outDir";

impl PublishProvider for LocalDirProvider {
    fn id(&self) -> &'static str {
        "local-dir"
    }

    fn display_name(&self) -> &'static str {
        "Local folder"
    }

    fn needs_credential(&self) -> bool {
        false
    }

    fn credential_env_var(&self) -> Option<String> {
        None
    }

    fn settings(&self) -> Vec<ProviderSettingInfo> {
        vec![ProviderSettingInfo {
            key: OUT_DIR_KEY.to_owned(),
            description: "absolute folder that receives the package and its receipt; it must not \
                sit inside any server root"
                .to_owned(),
        }]
    }

    fn validate_settings(&self, settings: &BTreeMap<String, String>) -> Result<(), CoreError> {
        for key in settings.keys() {
            if key != OUT_DIR_KEY {
                return Err(invalid(format!(
                    "the local-dir provider does not know the setting {key:?}"
                )));
            }
        }
        let dir = settings
            .get(OUT_DIR_KEY)
            .ok_or_else(|| invalid(format!("the local-dir provider needs `{OUT_DIR_KEY}`")))?;
        let path = Path::new(dir);
        if !path.is_absolute() {
            return Err(invalid(format!(
                "`{OUT_DIR_KEY}` must be an absolute path; got {dir:?}"
            )));
        }
        Ok(())
    }

    fn upload(
        &self,
        package: &ProviderPackageRef<'_>,
        settings: &BTreeMap<String, String>,
        _credential: Option<&str>,
    ) -> Result<UploadReceipt, CoreError> {
        let dir = settings
            .get(OUT_DIR_KEY)
            .ok_or_else(|| invalid(format!("the local-dir provider needs `{OUT_DIR_KEY}`")))?;
        std::fs::create_dir_all(dir).map_err(|source| CoreError::Io {
            path: dir.to_owned().into(),
            source,
        })?;
        let file_name = package
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("package.zip")
            .to_owned();
        let dest = Path::new(dir).join(&file_name);
        let copied = std::fs::copy(package.path, &dest).map_err(|source| CoreError::Io {
            path: dest.clone(),
            source,
        })?;
        if copied != package.size_bytes {
            return Err(CoreError::PublishUpload {
                provider: self.id().to_owned(),
                reason: format!(
                    "the copy is {copied} bytes but the package is {} bytes",
                    package.size_bytes
                ),
            });
        }
        let receipt = UploadReceipt {
            provider_id: self.id().to_owned(),
            reference: file_name,
            detail: Some(format!("copied to {}", dest.display())),
            at_ms: now_ms(),
        };
        let receipt_path = dest.with_extension("receipt.json");
        let bytes =
            serde_json::to_vec_pretty(&receipt).map_err(|e| CoreError::PublishStateCorrupt {
                path: receipt_path.clone(),
                reason: e.to_string(),
            })?;
        crate::fsops::atomic_write(&receipt_path, &bytes)?;
        Ok(receipt)
    }
}
