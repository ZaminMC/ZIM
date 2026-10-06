//! The PaperMC Fill API v3 client. Small and boring on purpose: three GET
//! requests, typed errors, no caching here (the daemon layers its own
//! short-lived cache where it matters — ARCH-REVIEW §18.6's "never
//! re-fetch on UI refresh" applies to *java inspection*, not to catalog
//! browsing, which hits the network only on explicit user action).
//!
//! Live endpoints (probed against the real service):
//! - `GET {base}/projects/{project}` → `{"project":{...},"versions":{"1.21":[…],…}}`
//! - `GET {base}/projects/{project}/versions/{version}` → `{"version":{"id":…,
//!   "java":{"version":{"minimum":21}},…},"builds":[34,33,…]}`
//! - `GET {base}/projects/{project}/versions/{version}/builds` → array of
//!   `{"id":34,"time":…,"channel":"…","downloads":{"server:default":{
//!    "name":"paper-1.21.11-34.jar","checksums":{"sha256":…},"size":…,"url":…}}}`

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

use crate::error::CoreError;

use super::USER_AGENT;

/// Metadata timeout for the JSON endpoints. Downloads own no overall
/// timeout (a 50 MiB jar on slow glass is legitimate) — they carry
/// progress and cancellation instead.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

pub struct FillClient {
    base: String,
    agent: ureq::Agent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuildDownload {
    /// File name as published, e.g. `paper-1.21.11-34.jar`.
    pub name: String,
    /// Published sha256 (lowercase hex).
    pub sha256: String,
    /// Published size in bytes, when the API states it.
    pub size: Option<u64>,
    /// Absolute download URL.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuildInfo {
    pub id: u64,
    /// `default`, `experimental`, … — the UI flags non-default channels.
    pub channel: String,
    /// ISO-8601 publish time as the API prints it (display-only).
    pub time: Option<String>,
    pub download: BuildDownload,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VersionInfo {
    pub id: String,
    /// The version's own Java requirement from the API (`java.version.minimum`);
    /// `None` when the API does not state one and the local table must decide.
    pub java_major: Option<u32>,
}

impl FillClient {
    pub fn new(base: &str) -> FillClient {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build();
        FillClient {
            base: base.trim_end_matches('/').to_owned(),
            agent,
        }
    }

    fn get_json(&self, path: &str) -> Result<serde_json::Value, CoreError> {
        let url = format!("{}{path}", self.base);
        let response = self
            .agent
            .get(&url)
            .set("Accept", "application/json")
            .call()
            .map_err(|e| http_error(&url, e))?;
        let body = response
            .into_string()
            .map_err(|e| CoreError::HttpTransport {
                url: url.clone(),
                message: e.to_string(),
            })?;
        serde_json::from_str(&body).map_err(|e| CoreError::HttpTransport {
            url,
            message: format!("response is not valid JSON: {e}"),
        })
    }

    /// All published versions for a project, newest first. The API groups
    /// versions under major keys in a JSON object (whose key order serde
    /// does not preserve), so flattening sorts by its own comparator.
    pub fn versions(&self, project: &str) -> Result<Vec<VersionInfo>, CoreError> {
        let value = self.get_json(&format!("/projects/{project}"))?;
        let map: BTreeMap<String, Vec<String>> = serde_json::from_value(
            value
                .get("versions")
                .cloned()
                .ok_or_else(|| malformed(project))?,
        )
        .map_err(|e| CoreError::HttpTransport {
            url: format!("{}/projects/{project}", self.base),
            message: format!("`versions` is not the expected shape: {e}"),
        })?;
        let mut out: Vec<VersionInfo> = map
            .into_values()
            .flatten()
            .map(|id| VersionInfo {
                id,
                java_major: None,
            })
            .collect();
        out.sort_by(|a, b| compare_versions(&b.id, &a.id));
        out.dedup_by(|a, b| a.id == b.id);
        Ok(out)
    }

    /// The Java major a version requires, API-first with the local table
    /// as the fallback (the API covers Paper-family knowledge better than
    /// any static table we maintain).
    pub fn version_java_major(
        &self,
        project: &str,
        version: &str,
    ) -> Result<Option<u32>, CoreError> {
        let value = self.get_json(&format!("/projects/{project}/versions/{version}"))?;
        let api = value
            .pointer("/version/java/version/minimum")
            .and_then(serde_json::Value::as_u64)
            .map(|m| m as u32);
        Ok(api.or_else(|| crate::java::required_major(version)))
    }

    /// A version's builds, newest first, keeping only builds that publish
    /// a `server:default` download (the one this project knows how to run).
    pub fn builds(&self, project: &str, version: &str) -> Result<Vec<BuildInfo>, CoreError> {
        let url = format!("{}/projects/{project}/versions/{version}/builds", self.base);
        let response = self
            .agent
            .get(&url)
            .set("Accept", "application/json")
            .call()
            .map_err(|e| http_error(&url, e))?;
        let body = response
            .into_string()
            .map_err(|e| CoreError::HttpTransport {
                url: url.clone(),
                message: e.to_string(),
            })?;
        let raw: Vec<RawBuild> =
            serde_json::from_str(&body).map_err(|e| CoreError::HttpTransport {
                url: url.clone(),
                message: format!("`builds` is not the expected shape: {e}"),
            })?;
        let mut out: Vec<BuildInfo> = raw
            .into_iter()
            .filter_map(|b| {
                let d = b.downloads.get("server:default")?;
                Some(BuildInfo {
                    id: b.id,
                    channel: b.channel,
                    time: Some(b.time).filter(|t| !t.is_empty()),
                    download: BuildDownload {
                        name: d.name.clone(),
                        sha256: d.checksums.sha256.to_ascii_lowercase(),
                        size: d.size,
                        url: d.url.clone(),
                    },
                })
            })
            .collect();
        out.sort_by_key(|b| std::cmp::Reverse(b.id));
        Ok(out)
    }
}

fn malformed(project: &str) -> CoreError {
    CoreError::HttpTransport {
        url: format!("projects/{project}"),
        message: "response is missing the `versions` object".to_owned(),
    }
}

/// Non-2xx answers are typed `Http` (the daemon maps 404 to
/// CATALOG_NOT_FOUND and everything else to CATALOG_UNAVAILABLE);
/// transport-level failures (DNS, refused, TLS) are `HttpTransport`.
fn http_error(url: &str, error: ureq::Error) -> CoreError {
    match error {
        ureq::Error::Status(status, response) => CoreError::Http {
            url: url.to_owned(),
            status,
            reason: response.status_text().to_owned(),
        },
        ureq::Error::Transport(t) => CoreError::HttpTransport {
            url: url.to_owned(),
            message: t.to_string(),
        },
    }
}

/// Newest-first comparator for Minecraft version strings: per-dot numeric
/// segments, and a final release sorts newer than its pre-releases
/// (`1.21.11` > `1.21.11-rc3`, `26.3` > `26.2`).
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut left = a.split('.');
    let mut right = b.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(l), Some(r)) => {
                let (ln, ls) = segment(l);
                let (rn, rs) = segment(r);
                match ln.cmp(&rn) {
                    Ordering::Equal => {}
                    other => return other,
                }
                match suffix_rank(ls).cmp(&suffix_rank(rs)) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
        }
    }
}

/// Split `"11-rc3"` into `(11, Some("rc3"))`, `"11"` into `(11, None)`.
fn segment(s: &str) -> (u64, Option<&str>) {
    match s.split_once('-') {
        Some((num, suffix)) => (num.parse().unwrap_or(0), Some(suffix)),
        None => (s.parse().unwrap_or(0), None),
    }
}

/// A final release ranks above any pre-release of the same number.
fn suffix_rank(suffix: Option<&str>) -> u8 {
    match suffix {
        None => 1,
        Some(_) => 0,
    }
}

// Wire shapes (only the fields this project reads).
#[derive(Deserialize)]
struct RawBuild {
    id: u64,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    time: String,
    downloads: BTreeMap<String, RawDownload>,
}

#[derive(Deserialize)]
struct RawDownload {
    name: String,
    checksums: RawChecksums,
    #[serde(default)]
    size: Option<u64>,
    url: String,
}

#[derive(Deserialize)]
struct RawChecksums {
    sha256: String,
}
