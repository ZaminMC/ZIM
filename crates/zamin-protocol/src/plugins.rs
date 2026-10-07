//! Plugin catalog methods (ADR-0012). The daemon speaks Modrinth on the
//! operator's behalf: search is loader-faceted, versions expose their
//! game_versions so the panel can display and pin, installs are jobs
//! (byte progress, cancellable), and the file system is the inventory —
//! the daemon keeps no plugin state beyond the jars themselves.

use serde::{Deserialize, Serialize};

use super::jobs::{Job, JobKind};

/// `plugins.search {serverId, query}` — Modrinth hits, restricted to the
/// loader family the server's directory speaks. No game-version facet:
/// the daemon never claims to know a registered server's version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsSearchParams {
    pub server_id: String,
    #[serde(default)]
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSearchHit {
    /// Modrinth's project id, as `plugins.install` expects it.
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub downloads: u64,
    /// Display-only; the panel renders initials when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    /// Loader slugs, for the honest "also runs on fabric" hint.
    pub loaders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsSearchResult {
    /// `plugins` or `mods` — where installs will land for this server.
    pub target: String,
    pub hits: Vec<PluginSearchHit>,
}

/// `plugins.versions {serverId, projectId}` — the pin list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsVersionsParams {
    pub server_id: String,
    pub project_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginVersionInfo {
    pub id: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_published: Option<String>,
    /// The primary file's name as published; absent when the version
    /// ships nothing installable (sha1-only files are not installable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsVersionsResult {
    pub target: String,
    pub versions: Vec<PluginVersionInfo>,
}

/// `plugins.installed {serverId}` — the directory is the inventory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsInstalledParams {
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub file_name: String,
    pub size_bytes: u64,
    pub modified_ms: i64,
    /// A symlink that leaves the server root: listed (the operator
    /// should know it exists), never deletable through the daemon.
    #[serde(default)]
    pub symlink_outside: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsInstalledResult {
    pub target: String,
    pub entries: Vec<InstalledPlugin>,
}

/// `plugins.install {serverId, projectId}` — latest-for-loader, or the
/// pinned `versionId`. A job: byte progress, cancellation, the works.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsInstallParams {
    pub server_id: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    /// Overwrite an installed file whose content differs from the
    /// published bytes (ADR-0012's update rule). The default refuses
    /// with `PLUGIN_EXISTS`; a re-install of the identical file is
    /// always allowed and short-circuits without a download.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsInstallResult {
    pub kind: JobKind,
    pub job: Job,
}

/// `plugins.delete {serverId, fileName}` — sanitized like every other
/// name from the wire; symlinked targets are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsDeleteParams {
    pub server_id: String,
    pub file_name: String,
}

/// `plugins.updates {serverId}` — the update check (ADR-0012's update
/// rule, read side). The disk's bytes identify each installed jar; the
/// catalog is asked, fresh, what it now publishes for that project. No
/// shadow state, no new client obligation: an explicit request that
/// costs one catalog round trip per recognized jar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsUpdatesParams {
    pub server_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginUpdateStatus {
    /// The file's digest matches the newest installable version's
    /// published digest.
    UpToDate,
    /// A newer (or different-loader) installable version exists; the
    /// entry carries the pin that applies it.
    UpdateAvailable,
    /// The catalog has no file with these bytes, or knows them but
    /// publishes nothing installable for this server's loader family —
    /// the operator action is "none through the panel", said plainly.
    Unmanaged,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateEntry {
    pub file_name: String,
    pub status: PluginUpdateStatus,
    /// Present when the catalog recognized the file's bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The installed version's display number, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    /// The catalog's newest installable version's display number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// The pin that applies the update: `plugins.install`'s `versionId`
    /// (with `replace`, per the update rule).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsUpdatesResult {
    /// `plugins` or `mods` — the directory that was checked.
    pub target: String,
    /// Sorted by file name, so clients render a stable order.
    pub entries: Vec<PluginUpdateEntry>,
}
