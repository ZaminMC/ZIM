//! The software catalog (ARCH-REVIEW §17.4): Paper-family servers in V1
//! are **data** — rows in a table plus a URL-speaking client — never a
//! trait hierarchy. A second family (Fabric/Forge) earns a tiny
//! `SoftwareSource` abstraction only when it actually lands.
//!
//! The live metadata source for Paper-family projects is the PaperMC Fill
//! API v3 (`https://fill.papermc.io/v3`); the legacy `api.papermc.io/v2`
//! is retired upstream and answers 410. The client's base URL is a
//! parameter, not a constant: tests point it at a local mock, and
//! air-gapped installs can point it at a mirror.

mod download;
mod fill;
mod templates;

pub use download::{download_to_dir, DownloadOptions, DownloadOutcome, DownloadProgress};
pub use fill::FillClient;
pub use templates::{stamp_template, template, templates, Template};

/// Honest request identification: the Fill API asks clients to say who
/// they are, and it is the right thing to do anyway.
pub const USER_AGENT: &str = concat!("zaminpanel/", env!("CARGO_PKG_VERSION"));

/// One catalog row: what the New Server flow offers. Everything a client
/// needs to pick software and fetch a jar is derived from these fields
/// through the Fill API.
#[derive(Debug, Clone, PartialEq)]
pub struct SoftwareEntry {
    /// Stable catalog id (`server.create`'s `project` parameter).
    pub id: &'static str,
    /// Human name the UI shows.
    pub name: &'static str,
    /// One-line description the UI shows under the name.
    pub description: &'static str,
    /// The Fill API project id these entries speak.
    pub project: &'static str,
}

/// The V1 catalog. Folia and Purpur share Paper's Fill API and creation
/// flow, so they are rows — adding them changes data, not code.
pub const CATALOG: &[SoftwareEntry] = &[
    SoftwareEntry {
        id: "paper",
        name: "Paper",
        description: "The high-performance Minecraft server — the default choice.",
        project: "paper",
    },
    SoftwareEntry {
        id: "purpur",
        name: "Purpur",
        description: "Paper fork with many more gameplay configuration options.",
        project: "purpur",
    },
    SoftwareEntry {
        id: "folia",
        name: "Folia",
        description: "Paper fork that regionizes the tick loop for very large servers.",
        project: "folia",
    },
];

/// The catalog entry for `id`, or `None` for anything the daemon does not
/// yet know how to create.
pub fn entry(id: &str) -> Option<&'static SoftwareEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests;
