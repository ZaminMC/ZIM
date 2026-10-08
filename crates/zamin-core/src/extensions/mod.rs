//! Extensions (founder §56/§57, ADR-0031): declared, never executed.
//!
//! An extension is a folder under the daemon's `extensions/` data dir
//! carrying one `zamin-extension.toml` manifest: who it is, and which
//! permissions it claims from a closed, deny-by-default vocabulary. This
//! module is the inventory and the validator — the machine-readable
//! half of the permission model. What extensions may *do* once the
//! execution model lands (context-menu entries, sidebar pages, server
//! integrations) is reserved by the same ADR: nothing here runs
//! extension code, and the panel says so.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// The manifest file name, one per extension folder.
pub const MANIFEST_FILE: &str = "zamin-extension.toml";

/// The permission vocabulary (ADR-0031). Two families, both
/// deny-by-default: a permission not on this list is a manifest
/// rejection, and a permission not declared is never granted.
///
/// `contribution:*` — what the extension may ADD to the panel once the
/// contribution model lands (§56's "may add" list).
/// `data:*` — what machine state it may touch when the execution model
/// lands; `servers.control` and `players.control` are the loud ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    // contributions (§56)
    #[serde(rename = "contribution:context-menu")]
    ContextMenu,
    #[serde(rename = "contribution:sidebar-page")]
    SidebarPage,
    #[serde(rename = "contribution:config-editor")]
    ConfigEditor,
    #[serde(rename = "contribution:server-integration")]
    ServerIntegration,
    #[serde(rename = "contribution:publish-provider")]
    PublishProvider,
    #[serde(rename = "contribution:software-support")]
    SoftwareSupport,
    #[serde(rename = "contribution:dutchmen-tool")]
    DutchmenTool,
    #[serde(rename = "contribution:marketplace")]
    Marketplace,
    // data access (§56: "should NOT receive unrestricted access")
    #[serde(rename = "data:servers.read")]
    ServersRead,
    #[serde(rename = "data:servers.control")]
    ServersControl,
    #[serde(rename = "data:files.read")]
    FilesRead,
    #[serde(rename = "data:files.write")]
    FilesWrite,
    #[serde(rename = "data:console.read")]
    ConsoleRead,
    #[serde(rename = "data:console.send")]
    ConsoleSend,
    #[serde(rename = "data:players.read")]
    PlayersRead,
    #[serde(rename = "data:players.control")]
    PlayersControl,
}

impl Permission {
    /// The wire spelling — also the manifest spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::ContextMenu => "contribution:context-menu",
            Permission::SidebarPage => "contribution:sidebar-page",
            Permission::ConfigEditor => "contribution:config-editor",
            Permission::ServerIntegration => "contribution:server-integration",
            Permission::PublishProvider => "contribution:publish-provider",
            Permission::SoftwareSupport => "contribution:software-support",
            Permission::DutchmenTool => "contribution:dutchmen-tool",
            Permission::Marketplace => "contribution:marketplace",
            Permission::ServersRead => "data:servers.read",
            Permission::ServersControl => "data:servers.control",
            Permission::FilesRead => "data:files.read",
            Permission::FilesWrite => "data:files.write",
            Permission::ConsoleRead => "data:console.read",
            Permission::ConsoleSend => "data:console.send",
            Permission::PlayersRead => "data:players.read",
            Permission::PlayersControl => "data:players.control",
        }
    }

    /// Parse a manifest string. Unknown → Err (deny-by-default: an
    /// unrecognized permission is a rejection, never a shrug).
    pub fn parse(raw: &str) -> Result<Permission, CoreError> {
        const ALL: [Permission; 16] = [
            Permission::ContextMenu,
            Permission::SidebarPage,
            Permission::ConfigEditor,
            Permission::ServerIntegration,
            Permission::PublishProvider,
            Permission::SoftwareSupport,
            Permission::DutchmenTool,
            Permission::Marketplace,
            Permission::ServersRead,
            Permission::ServersControl,
            Permission::FilesRead,
            Permission::FilesWrite,
            Permission::ConsoleRead,
            Permission::ConsoleSend,
            Permission::PlayersRead,
            Permission::PlayersControl,
        ];
        ALL.into_iter().find(|p| p.as_str() == raw).ok_or_else(|| {
            CoreError::InvalidExtensionPermission {
                permission: raw.to_string(),
            }
        })
    }
}

/// The extension's self-description (§57: "extensions must declare
/// their permissions").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionManifest {
    /// Machine identity: `[a-z0-9][a-z0-9_-]{0,63}`.
    pub id: String,
    /// Human name, non-empty, ≤ 64 chars.
    pub name: String,
    /// The extension's own version string, non-empty, ≤ 32 chars.
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<Permission>,
}

impl ExtensionManifest {
    /// Validate the self-declaration. Rules that keep ids and claims
    /// honest: slug ids (mirrors ServerId's discipline), bounded names,
    /// duplicate permissions folded, nothing invented.
    pub fn validate(mut self) -> Result<ExtensionManifest, CoreError> {
        let bytes = self.id.as_bytes();
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 64
            && bytes
                .first()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_');
        if !id_ok {
            return Err(CoreError::InvalidExtensionId {
                id: self.id.clone(),
            });
        }
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(CoreError::InvalidExtensionManifest {
                id: self.id.clone(),
                reason: "the name must be 1–64 characters".to_string(),
            });
        }
        self.name = name.to_string();
        let version = self.version.trim();
        if version.is_empty() || version.len() > 32 {
            return Err(CoreError::InvalidExtensionManifest {
                id: self.id.clone(),
                reason: "the version must be 1–32 characters".to_string(),
            });
        }
        self.version = version.to_string();
        if let Some(description) = &self.description {
            let trimmed = description.trim();
            if trimmed.chars().count() > 200 {
                return Err(CoreError::InvalidExtensionManifest {
                    id: self.id.clone(),
                    reason: "the description must be at most 200 characters".to_string(),
                });
            }
            self.description = Some(trimmed.to_string());
        }
        self.permissions.sort();
        self.permissions.dedup();
        Ok(self)
    }
}

/// One folder under the extensions dir: either a validated extension or
/// a named problem — a broken manifest is evidence, never a silent skip
/// (§82: the inventory answers for everything it saw). The directory
/// name rides along in both arms: it is the evidence the page shows.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtensionListing {
    Valid {
        directory: String,
        manifest: Box<ExtensionManifest>,
    },
    Invalid {
        directory: String,
        reason: String,
    },
}

/// Read and validate the manifest in one extension folder. Symlinks are
/// refused up front (ADR-0009's rule, kept here too).
pub fn load_manifest(dir: &Path) -> Result<ExtensionManifest, CoreError> {
    let meta = std::fs::symlink_metadata(dir).map_err(|e| CoreError::ExtensionDirUnreadable {
        path: dir.to_path_buf(),
        reason: e.to_string(),
    })?;
    if meta.file_type().is_symlink() {
        return Err(CoreError::ExtensionDirUnreadable {
            path: dir.to_path_buf(),
            reason: "symlinked extension folders are never followed".to_string(),
        });
    }
    let raw =
        std::fs::read(dir.join(MANIFEST_FILE)).map_err(|e| CoreError::ExtensionDirUnreadable {
            path: dir.to_path_buf(),
            reason: format!("{} is unreadable: {e}", MANIFEST_FILE),
        })?;
    let text = std::str::from_utf8(&raw).map_err(|e| CoreError::ExtensionDirUnreadable {
        path: dir.to_path_buf(),
        reason: format!("{} is not UTF-8: {e}", MANIFEST_FILE),
    })?;
    let parsed: ExtensionManifest =
        toml::from_str(text).map_err(|e| CoreError::ExtensionDirUnreadable {
            path: dir.to_path_buf(),
            reason: format!("{} did not parse: {e}", MANIFEST_FILE),
        })?;
    parsed.validate()
}

/// Walk the extensions dir (one level, no symlinks, deterministic
/// order). A missing dir is an empty inventory — there is nothing to
/// answer for; everything found IS answered for.
pub fn list_extensions(dir: &Path) -> Vec<ExtensionListing> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && !e.file_name().to_string_lossy().starts_with('.')
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| match load_manifest(&dir.join(&name)) {
            Ok(manifest) => ExtensionListing::Valid {
                directory: name,
                manifest: Box::new(manifest),
            },
            Err(e) => ExtensionListing::Invalid {
                directory: name,
                reason: e.to_string(),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests;
