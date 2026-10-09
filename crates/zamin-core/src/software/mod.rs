//! The software catalog (ARCH-REVIEW §17.4): catalog rows are **data** —
//! a table plus a URL-speaking client per family — never a trait
//! hierarchy. The second family (Fabric) landed as the tiny
//! `SoftwareSource` enum §17.8 promised: two arms, not a trait zoo.
//!
//! The live metadata source for the Paper family is the PaperMC Fill
//! API v3 (`https://fill.papermc.io/v3`); the legacy `api.papermc.io/v2`
//! is retired upstream and answers 410. The Fabric family speaks the
//! FabricMC meta API v2 (`https://meta.fabricmc.net`). Every client's
//! base URL is a parameter, not a constant: tests point them at local
//! mocks, and air-gapped installs can point them at mirrors.

mod download;
mod fabric;
mod fill;
mod templates;

pub use download::{
    download_to_dir, download_to_staging, download_verified, DownloadOptions, DownloadOutcome,
    DownloadProgress, Verified,
};
pub use fabric::{FabricMetaClient, FabricServerJar, MetaVersion};
pub use fill::FillClient;
pub use templates::{stamp_template, template, templates, Template, DEFAULT_TEMPLATE_ID};

/// Honest request identification: the Fill API asks clients to say who
/// they are, and it is the right thing to do anyway.
pub const USER_AGENT: &str = concat!("zim/", env!("CARGO_PKG_VERSION"));

/// Which upstream a catalog row speaks. An enum of two, exactly as wide
/// as the families that exist — a third family edits this type and adds
/// a client module, nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftwareSource {
    /// The PaperMC Fill API v3: projects, versions, builds with
    /// published sha256 checksums.
    Fill,
    /// The FabricMC meta API v2: version lists plus a direct
    /// launcher-jar endpoint that publishes no checksums.
    FabricMeta,
}

impl SoftwareSource {
    /// The wire spelling (protocol `catalog.list` entries carry it).
    pub fn as_str(self) -> &'static str {
        match self {
            SoftwareSource::Fill => "fill",
            SoftwareSource::FabricMeta => "fabric-meta",
        }
    }
}

/// One catalog row: what the New Server flow offers. Everything a client
/// needs to pick software and fetch a jar is derived from these fields
/// through the row's family client.
#[derive(Debug, Clone, PartialEq)]
pub struct SoftwareEntry {
    /// Stable catalog id (`server.create`'s `project` parameter).
    pub id: &'static str,
    /// Human name the UI shows.
    pub name: &'static str,
    /// One-line description the UI shows under the name.
    pub description: &'static str,
    /// The upstream project id the family client speaks.
    pub project: &'static str,
    /// Which API family this row speaks.
    pub source: SoftwareSource,
}

/// The catalog. Paper, Purpur and Folia share the Fill API and creation
/// flow, so they are rows — adding them changes data, not code. Fabric
/// is the second family and brought the enum with it.
pub const CATALOG: &[SoftwareEntry] = &[
    SoftwareEntry {
        id: "paper",
        name: "Paper",
        description: "The high-performance Minecraft server — the default choice.",
        project: "paper",
        source: SoftwareSource::Fill,
    },
    SoftwareEntry {
        id: "purpur",
        name: "Purpur",
        description: "Paper fork with many more gameplay configuration options.",
        project: "purpur",
        source: SoftwareSource::Fill,
    },
    SoftwareEntry {
        id: "folia",
        name: "Folia",
        description: "Paper fork that regionizes the tick loop for very large servers.",
        project: "folia",
        source: SoftwareSource::Fill,
    },
    SoftwareEntry {
        id: "fabric",
        name: "Fabric",
        description: "The lightweight mod loader family — mods live in mods/, the Plugins tab already serves them.",
        project: "fabric",
        source: SoftwareSource::FabricMeta,
    },
];

/// The catalog entry for `id`, or `None` for anything the daemon does not
/// yet know how to create.
pub fn entry(id: &str) -> Option<&'static SoftwareEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests;
