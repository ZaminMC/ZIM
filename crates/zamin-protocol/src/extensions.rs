//! Extensions on the wire (founder §56/§57, ADR-0031): the daemon's
//! inventory of installed extensions and the problems it found while
//! reading them. This is the declaration half of the permission model —
//! what an extension claims, never what it may already do. The
//! execution/contribution model is reserved by the same ADR, and the
//! panel says so.

use serde::{Deserialize, Serialize};

/// One valid extension: who it is and which permissions it claims from
/// the closed, deny-by-default vocabulary (ADR-0031). Permissions ride
/// as their wire strings (`contribution:*` / `data:*`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionView {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub permissions: Vec<String>,
    /// The folder the manifest was read from — evidence, always.
    pub directory: String,
}

/// A folder under the extensions dir the daemon could not answer for:
/// named with its reason, never silently skipped (§82).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionProblem {
    pub directory: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionsListResult {
    /// The scanned root, so the page can say where extensions live.
    pub directory: String,
    pub extensions: Vec<ExtensionView>,
    pub problems: Vec<ExtensionProblem>,
    /// Stated so the page cannot overpromise: the execution and
    /// contribution model is reserved (ADR-0031).
    pub contributions_active: bool,
}
