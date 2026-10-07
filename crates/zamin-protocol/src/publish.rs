//! Publish (founder vision §40–47, §74, ADR-0017): the creator packages
//! and publishes a server — a chosen selection of its files — through a
//! provider interface, with a diff against the previous publication and
//! a security scan in front of the packaging.
//!
//! Scoping note that travels with the founder's document: the AI part is
//! explicitly ignored, rooms reserved. §43's Dutchmen-generated changelog
//! is therefore NOT on this wire — the changelog is a plain string the
//! operator edits; a future `publish.changelog.draft` room is named in
//! ADR-0017 and stays empty until a real demand names it.
//!
//! Everything here is pure data and codec, like the rest of the crate.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::ProtocolError;
use crate::jobs::Job;

/// One file-picking rule (founder §41): a whole folder, one exact file,
/// or a glob for specific files inside folders. Paths are server-root
/// relative, `/`-separated, and never escape the root — validation is a
/// core concern so the daemon and every future client refuse garbage
/// identically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SelectionRule {
    /// The folder at `path`, recursively.
    Folder { path: String },
    /// The single file at `path`.
    File { path: String },
    /// Glob with `*` / `?` inside a segment and `**` spanning segments,
    /// e.g. `plugins/**/*.yml`.
    Glob { pattern: String },
}

/// What a publish packages: the include rules and the exclude rules.
/// Excludes win over includes. An empty include list selects NOTHING —
/// the founder's rule that the whole server directory is never blindly
/// packaged is structural, not a warning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PublishSelection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<SelectionRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excludes: Vec<SelectionRule>,
}

/// The per-server publish configuration: the founder's §40 form. The
/// provider is addressed by id (`publish.providers.list` enumerates the
/// choices); provider settings are non-secret strings (e.g. a local
/// output folder). Credentials are NEVER here — §47 keeps them in the
/// OS's secure storage or an environment channel, never in project
/// configuration, packages, or logs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishConfig {
    #[serde(default)]
    pub selection: PublishSelection,
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_settings: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub changelog: String,
}

impl Default for PublishConfig {
    fn default() -> Self {
        PublishConfig {
            selection: PublishSelection::default(),
            provider_id: "archive".to_owned(),
            provider_settings: BTreeMap::new(),
            title: String::new(),
            description: String::new(),
            version: String::new(),
            changelog: String::new(),
        }
    }
}

/// A file's state against the previous publication (founder §42's
/// M / A / D inspection; `unchanged` completes the set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileDiffStatus {
    Added,
    Modified,
    Removed,
    Unchanged,
}

/// One row of the §42 diff listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiffEntry {
    pub path: String,
    pub status: FileDiffStatus,
    /// Current size on disk; for `removed` rows the size the file had
    /// when it was last published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Present for rows the current selection still holds (digest of the
    /// bytes on disk); absent for removed rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
}

/// The §42 change counter ("12 files changed").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffCounts {
    pub added: u64,
    pub modified: u64,
    pub removed: u64,
    pub unchanged: u64,
    /// added + modified + removed — the number the Publish button wears.
    pub changed: u64,
}

/// How severe a scan finding is. The scanner is a safety mechanism, not
/// a guarantee (founder §46) — severity orders the operator's attention,
/// it does not certify the absence of secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretSeverity {
    Critical,
    High,
    Medium,
    Low,
}

/// One detection: where, what kind, how bad, and a REDACTED excerpt.
/// The excerpt never carries the secret itself (founder §47: credentials
/// must not reach logs, packages, or prompts — the finding is evidence
/// of a pattern, not a copy of the value).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretFinding {
    /// Server-root relative path.
    pub file: String,
    /// 1-based line for in-file detections; 0 marks a file-level finding
    /// (a sensitive filename, or the DiscordSRV advisory).
    pub line: u32,
    /// Stable kind id, e.g. `discord-bot-token`, `config-secret-key`,
    /// `sensitive-filename`, `high-entropy-string`. Open vocabulary —
    /// the scanner is extensible and readers stay tolerant.
    pub kind: String,
    pub severity: SecretSeverity,
    /// Redacted human context: a key name, or the token's first few
    /// characters plus its length.
    pub excerpt: String,
    /// The detector that spoke, e.g. `discord-bot-token`, `config-key`,
    /// `high-entropy`, `sensitive-filename`, `discord-srv-advisory`.
    pub detector: String,
    /// True when an operator has reviewed this (file, kind) pair — the
    /// §46 mechanism for false positives. Reviewed findings stop
    /// blocking; they stay visible.
    pub reviewed: bool,
}

/// The result of scanning the current selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub findings: Vec<SecretFinding>,
    pub files_scanned: u64,
    /// Files too large to line-scan (binary-heavy or enormous), counted
    /// honestly rather than silently ignored.
    pub files_skipped: u64,
}

/// A recorded false-positive review (founder §46): the (file, kind) pair
/// an operator has marked reviewed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEntry {
    pub file: String,
    pub kind: String,
}

/// The last publication's summary, as `preview` and `state` echo it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationSummary {
    pub published_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub provider_id: String,
    pub package_sha512: String,
    pub package_bytes: u64,
    pub file_count: u64,
}

/// What a provider handed back after an upload. `reference` is the
/// provider's own identifier for the artifact (a file name, an upload
/// id — provider-defined). Credentials never appear here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadReceipt {
    pub provider_id: String,
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub at_ms: i64,
}

/// One provider as `publish.providers.list` describes it — enough for a
/// UI to render the choice honestly without any marketplace behavior
/// being hardcoded anywhere (founder §40).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub needs_credential: bool,
    /// The environment variable a credential rides in, when the provider
    /// needs one (§47: the daemon never stores credentials).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_env_var: Option<String>,
    pub settings: Vec<ProviderSettingInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingInfo {
    pub key: String,
    pub description: String,
}

/// `publish.config.get {serverId}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishConfigGetParams {
    pub server_id: String,
}

/// `publish.config.set {serverId, config}` — a full, validated replace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishConfigSetParams {
    pub server_id: String,
    pub config: PublishConfig,
}

/// `publish.preview {serverId}` — selection resolution, the §42 diff,
/// and the §44 scan in one read-only answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishPreviewParams {
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishPreviewResult {
    pub server_id: String,
    /// The config as it stands (defaults when never set).
    pub config: PublishConfig,
    pub files: Vec<FileDiffEntry>,
    pub counts: DiffCounts,
    pub scan: ScanReport,
    /// Findings that would refuse an execute right now (unreviewed and
    /// above `low`) — the number the security check wears.
    pub blocking_count: u64,
    /// How much the current selection holds, so the packaging stage can
    /// be priced before it runs.
    pub selected_files: u64,
    pub selected_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_publication: Option<PublicationSummary>,
}

/// `publish.execute {serverId, confirmUnsafe?}` — runs as a job (§74:
/// preparing → scanning → packaging → uploading → completed). Setting
/// `confirmUnsafe` is the "Publish Anyway" of founder §45: an explicit,
/// logged override of the security gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishExecuteParams {
    pub server_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm_unsafe: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishExecuteResult {
    pub job: Job,
}

/// `publish.state {serverId}` — the last publication and its receipt,
/// plus whether the packaged artifact is still on the daemon's disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishStateParams {
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishStateResult {
    pub server_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_publication: Option<PublicationSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<UploadReceipt>,
    pub package_present: bool,
}

/// `publish.review.set {serverId, file, kind, reviewed}` — mark or clear
/// a false-positive review (§46). Answers with the fresh preview so the
/// security panel re-renders from one round trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishReviewSetParams {
    pub server_id: String,
    pub file: String,
    pub kind: String,
    pub reviewed: bool,
}

/// `publish.providers.list` — no params; the list is daemon-global.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersListParams {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersListResult {
    pub providers: Vec<ProviderInfo>,
}

/// A publish job failed the security gate. The context carries the
/// blocking count and each blocking file (kind included, secrets never).
pub fn secrets_detected_error(blocking: &[&SecretFinding]) -> ProtocolError {
    let files: Vec<String> = blocking
        .iter()
        .map(|f| format!("{} ({})", f.file, f.kind))
        .collect();
    ProtocolError::new(
        crate::error::ErrorCode::PublishSecretsDetected,
        format!(
            "the security scan found {} unreviewed finding(s); review them, exclude the files, or publish anyway explicitly",
            blocking.len()
        ),
    )
    .with_context("blockingCount", blocking.len())
    .with_context("files", files)
    .with_remediation(&["review", "exclude-file", "publish-anyway", "cancel"])
}

/// A publish job was asked to run with nothing selected. The founder's
/// §41 rule is structural: empty include list means refuse, never
/// "package everything".
pub fn nothing_selected_error() -> ProtocolError {
    ProtocolError::new(
        crate::error::ErrorCode::PublishNothingSelected,
        "the publish selection includes no files; add include rules to publish.config.set",
    )
    .with_remediation(&["configure-selection"])
}
