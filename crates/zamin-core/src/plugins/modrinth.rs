// The Modrinth v2 client (ADR-0012). Same shape as the Fill client:
// small, boring, typed errors, no caching — catalog browsing hits the
// network only on explicit user action. The base URL is a parameter:
// tests point it at a local mock, air-gapped installs at a mirror.
//
// Live endpoints (probed against the real service):
// - `GET {base}/v2/search?query=…&limit=…&facets=[["project_type:mod"],
//   ["categories:paper","categories:spigot",…]]` → `{"hits":[{project_id,
//   slug, title, description, downloads, icon_url, loaders,…}]}`
// - `GET {base}/v2/project/{id}/version` → array of
//   `{"id":…,"version_number":…,"game_versions":["1.21.1",…],
//    "loaders":["paper"],"date_published":…,
//    "files":[{"url":…,"filename":…,"primary":true,"size":…,
//              "hashes":{"sha1":…,"sha512":…}}]}`
// - `GET {base}/v2/version_file/{sha512}?hashes=sha512` → one object of
//   that same shape (the version whose file carries the digest), or 404
//   when none does — the update check's "what is this jar?" question.

use std::time::Duration;

use serde::Deserialize;

use crate::error::CoreError;
use crate::http::idempotent_get;
use crate::software::USER_AGENT;

/// Metadata timeout for the JSON endpoints. Downloads own no overall
/// timeout (a 50 MiB jar on slow glass is legitimate) — they carry
/// progress and cancellation instead, via the shared downloader.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

pub struct ModrinthClient {
    base: String,
    agent: ureq::Agent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// Modrinth's base62 project id (`plugins.install`'s `projectId`).
    pub project_id: String,
    /// URL slug, for display and debugging.
    pub slug: String,
    pub title: String,
    pub description: String,
    pub downloads: u64,
    /// Icon URL when the project has one; the panel renders initials
    /// otherwise. Never dereferenced by the daemon.
    pub icon_url: Option<String>,
    /// Loader slugs the project supports (`paper`, `fabric`, …).
    pub loaders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VersionFile {
    /// Absolute download URL.
    pub url: String,
    /// Published file name — untrusted until it passes `safe_file_name`.
    pub filename: String,
    /// Lowercase hex sha512, as published by the API.
    pub sha512: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectVersion {
    pub id: String,
    /// The owning project's id — the version-from-hash answer needs it
    /// (the update check learns "which plugin is this jar?" from it).
    pub project_id: String,
    /// The version's display number (`1.2.3+mc1.21.4`).
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    /// ISO-8601 publish time as the API prints it (display-only).
    pub date_published: Option<String>,
    /// The file to download (the primary one), or `None` when a version
    /// ships only auxiliary files — such versions are skipped by the
    /// caller, never half-installed.
    pub file: Option<VersionFile>,
}

impl ModrinthClient {
    /// `base` is origin-only (`https://api.modrinth.com`) — every method
    /// appends the `/v2/...` path segment itself. Passing a base that
    /// already carries `/v2` composes `/v2/v2/...` requests.
    pub fn new(base: &str) -> ModrinthClient {
        ModrinthClient {
            base: base.trim_end_matches('/').to_owned(),
            agent: ureq::AgentBuilder::new()
                .timeout_read(REQUEST_TIMEOUT)
                .timeout_write(REQUEST_TIMEOUT)
                .user_agent(USER_AGENT)
                .build(),
        }
    }

    /// Search plugins/mods, restricted to one loader facet group (the
    /// group is OR — `["paper","spigot"]` matches either). The query may
    /// be empty: the default `relevance` index still returns usable hits.
    pub fn search(
        &self,
        query: &str,
        loaders: &[&str],
        limit: u32,
    ) -> Result<Vec<SearchHit>, CoreError> {
        // Facets are JSON-in-query-string: an array of groups, each group
        // an array of AND-ed facets. Loaders share one group (OR).
        let facets = serde_json::json!([
            ["project_type:mod"],
            loaders
                .iter()
                .map(|l| format!("categories:{l}"))
                .collect::<Vec<_>>(),
        ])
        .to_string();
        let url = format!(
            "{}/v2/search?query={}&limit={}&facets={}",
            self.base,
            urlencode(query),
            limit.clamp(1, 50),
            urlencode(&facets),
        );
        let body = self.get_json(&url)?;
        #[derive(Deserialize)]
        struct SearchResponse {
            hits: Vec<RawHit>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct RawHit {
            project_id: String,
            slug: String,
            title: String,
            description: String,
            #[serde(default)]
            downloads: u64,
            #[serde(default)]
            icon_url: Option<String>,
            #[serde(default)]
            loaders: Vec<String>,
        }
        let parsed: SearchResponse =
            serde_json::from_str(&body).map_err(|e| CoreError::HttpTransport {
                url: url.clone(),
                message: format!("search response is not the shape the client speaks: {e}"),
            })?;
        Ok(parsed
            .hits
            .into_iter()
            .map(|h| SearchHit {
                project_id: h.project_id,
                slug: h.slug,
                title: h.title,
                description: h.description,
                downloads: h.downloads,
                icon_url: h.icon_url,
                loaders: h.loaders,
            })
            .collect())
    }

    /// All versions of a project, newest first as the API returns them.
    /// Loader filtering is the caller's business (the client stays a
    /// dumb pipe; the engine owns the policy).
    pub fn versions(&self, project_id: &str) -> Result<Vec<ProjectVersion>, CoreError> {
        let url = format!("{}/v2/project/{}/version", self.base, urlencode(project_id));
        let body = self.get_json(&url)?;
        let parsed: Vec<RawVersion> =
            serde_json::from_str(&body).map_err(|e| CoreError::HttpTransport {
                url: url.clone(),
                message: format!("versions response is not the shape the client speaks: {e}"),
            })?;
        Ok(parsed.into_iter().map(raw_to_version).collect())
    }

    /// The version whose file carries this exact sha512, or `None` when
    /// the catalog publishes no such file. A miss is a normal answer,
    /// not an error: a jar dropped on disk by hand is unknown to the
    /// catalog by definition.
    pub fn version_from_sha512(&self, sha512: &str) -> Result<Option<ProjectVersion>, CoreError> {
        let url = format!(
            "{}/v2/version_file/{}?hashes=sha512",
            self.base,
            urlencode(sha512)
        );
        match idempotent_get(|| self.agent.get(&url).call().map_err(Box::new)) {
            Ok(response) => {
                let body = response
                    .into_string()
                    .map_err(|e| CoreError::HttpTransport {
                        url: url.clone(),
                        message: e.to_string(),
                    })?;
                let parsed: RawVersion =
                    serde_json::from_str(&body).map_err(|e| CoreError::HttpTransport {
                        url: url.clone(),
                        message: format!(
                            "version_file response is not the shape the client speaks: {e}"
                        ),
                    })?;
                Ok(Some(raw_to_version(parsed)))
            }
            Err(error) => match *error {
                ureq::Error::Status(404, _) => Ok(None),
                error => Err(http_error(&url, error)),
            },
        }
    }

    fn get_json(&self, url: &str) -> Result<String, CoreError> {
        match idempotent_get(|| self.agent.get(url).call().map_err(Box::new)) {
            Ok(response) => response
                .into_string()
                .map_err(|e| CoreError::HttpTransport {
                    url: url.to_owned(),
                    message: e.to_string(),
                }),
            Err(error) => Err(http_error(url, *error)),
        }
    }
}

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

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct RawVersion {
    id: String,
    #[serde(default)]
    project_id: String,
    version_number: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    date_published: Option<String>,
    #[serde(default)]
    files: Vec<RawFile>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
struct RawFile {
    url: String,
    filename: String,
    size: Option<u64>,
    hashes: RawHashes,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
struct RawHashes {
    #[serde(default)]
    sha512: Option<String>,
}

fn raw_to_version(v: RawVersion) -> ProjectVersion {
    let file = v
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| v.files.first())
        .cloned()
        .filter(|f| f.hashes.sha512.is_some())
        .map(|f| VersionFile {
            url: f.url,
            filename: f.filename,
            sha512: f.hashes.sha512.unwrap_or_default(),
            size: f.size,
        });
    ProjectVersion {
        id: v.id,
        project_id: v.project_id,
        version_number: v.version_number,
        game_versions: v.game_versions,
        loaders: v.loaders,
        date_published: v.date_published,
        file,
    }
}

/// Percent-encode every byte outside the RFC 3986 unreserved set — the
/// facets JSON carries brackets and quotes, queries carry spaces.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
