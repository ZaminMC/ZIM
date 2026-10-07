//! Software catalog and server creation (Phase 6). The catalog is data on
//! both sides of the wire: entries, versions, builds with published
//! checksums; `server.create` turns a choice into a running-ready server
//! directory as a job — the zero-manual-JAR path (protocol spec §7b).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `catalog.list` — the software the daemon knows how to create.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogListResult {
    pub entries: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Which upstream API family this entry speaks: `"fill"` (the
    /// PaperMC Fill family — builds with published sha256 digests) or
    /// `"fabric-meta"` (the FabricMC meta family — version lists plus a
    /// launcher-jar endpoint that publishes no checksums). Additive in
    /// protocol v0; the family decides what `catalog.builds` returns and
    /// which `server.create` parameters apply.
    pub source: String,
}

/// `catalog.versions {project}` — newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVersionsParams {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVersionsResult {
    pub project: String,
    pub versions: Vec<CatalogVersion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVersion {
    pub id: String,
    /// The Java major the software requires, when known from the catalog
    /// itself. Absent means "the daemon will decide at creation time".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_major: Option<u32>,
}

/// `catalog.builds {project, version}` — builds newest first; the chosen
/// version's Java requirement rides along so clients can offer a matching
/// runtime in the same breath.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogBuildsParams {
    pub project: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogBuildsResult {
    pub project: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_major: Option<u32>,
    pub builds: Vec<CatalogBuild>,
    /// Fabric-family rows carry their equivalent of builds here: the
    /// stable loader versions, newest first, one of which
    /// `server.create`'s `loader` parameter pins (omit = newest). `None`
    /// for the Fill family; `builds` is empty when `loaders` is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaders: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogBuild {
    pub id: u64,
    /// `default`, `experimental`, … — clients flag non-default channels.
    pub channel: String,
    /// Publish time as the catalog prints it (display-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    pub download: CatalogDownload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDownload {
    pub name: String,
    /// Published sha256, lowercase hex. The daemon verifies every download.
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// The daemon downloads this URL itself; clients never fetch it.
    pub url: String,
}

/// `server.create` — a job: download the jar, stamp the template, write
/// the per-server configuration, register. The server appears in
/// `server.list` (and via the `registered` event) when the job succeeds;
/// a failed or cancelled job leaves nothing behind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCreateParams {
    pub request_id: Uuid,
    pub server_id: String,
    /// Defaults to the server id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// A `catalog.list` entry id.
    pub project: String,
    pub version: String,
    /// Omit for the newest build. The Fabric family has no numeric
    /// builds: send `loader` instead (a `catalog.builds` `loaders` id),
    /// or omit for the newest stable one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<u64>,
    /// The Fabric loader version to pin, from `catalog.builds`'
    /// `loaders` list. Omit for the newest stable loader; ignored (and
    /// never required) by the Fill family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<String>,
    /// Omit for the default creation template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    /// Desired port; written into the server's settings and its stamped
    /// `server.properties`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Explicit `java` executable for the new server. Omit to let the
    /// daemon pick (system candidates, then its managed runtimes) at
    /// start time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCreateResult {
    pub kind: crate::jobs::JobKind,
    pub job: crate::jobs::Job,
}
