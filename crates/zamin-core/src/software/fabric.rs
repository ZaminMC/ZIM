//! The FabricMC meta API v2 client — the second software family, the one
//! ARCH-REVIEW §17.8 said would earn a "tiny `SoftwareSource`" when it
//! actually lands. It landed as a two-arm enum (`SoftwareSource`), not a
//! trait zoo, and Fabric is the honest first extension: its meta API
//! hands over a ready-to-run server launcher jar, no installer step, no
//! build matrix.
//!
//! Wire shapes (observed against `meta.fabricmc.net/v2`; the client's
//! base URL is a parameter like every other catalog — tests point it at
//! a local mock, air-gapped installs at a mirror):
//! - `GET {base}/v2/versions/game` → `[{"version":"1.21.5","stable":true},…]`
//! - `GET {base}/v2/versions/loader` → `[{"version":"0.16.14","stable":true,…},…]`
//! - `GET {base}/v2/versions/installer` → `[{"version":"1.0.1","stable":true,…},…]`
//! - `GET {base}/v2/versions/loader/{game}/{loader}/{installer}/server/jar`
//!   → the launcher jar bytes
//!
//! All three lists arrive newest-first from the API and are passed
//! through in that order — no local re-sorting (unlike the Fill client,
//! whose version map loses its order to JSON objects). The jar endpoint
//! publishes **no checksum**: a Fabric download is verified by TLS and
//! its computed sha256 is recorded and reported (the downloader always
//! hashes the stream), while Paper-family downloads verify a published
//! digest. The difference is stated here because it is real.

use serde::Deserialize;

use crate::error::CoreError;

use super::fill::http_error;
use super::USER_AGENT;

/// Metadata timeout for the JSON endpoints — the Fill client's rule:
/// downloads own no overall timeout, they carry progress and
/// cancellation instead.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

pub struct FabricMetaClient {
    base: String,
    agent: ureq::Agent,
}

/// One row of a meta version list: the id and whether upstream considers
/// it stable (snapshots and pre-releases are not).
#[derive(Debug, Clone, PartialEq)]
pub struct MetaVersion {
    pub version: String,
    pub stable: bool,
}

/// The resolved download for a Fabric server: everything the daemon's
/// creation job needs to stream the launcher jar into place.
#[derive(Debug, Clone, PartialEq)]
pub struct FabricServerJar {
    /// File name in Fabric's own convention, e.g.
    /// `fabric-server-mc.1.21.4-loader.0.16.14-launcher.1.0.1.jar`.
    pub name: String,
    /// Absolute download URL (the daemon fetches it itself).
    pub url: String,
    /// The loader version this jar pins (resolved when the caller passed
    /// `None`).
    pub loader: String,
    /// The installer version the launcher was built with.
    pub installer: String,
}

impl FabricMetaClient {
    pub fn new(base: &str) -> FabricMetaClient {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build();
        FabricMetaClient {
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

    fn list(&self, kind: &str) -> Result<Vec<MetaVersion>, CoreError> {
        let url_path = format!("/v2/versions/{kind}");
        let raw: Vec<RawMetaVersion> = serde_json::from_value(self.get_json(&url_path)?)
            .map_err(|e| CoreError::HttpTransport {
                url: format!("{}{url_path}", self.base),
                message: format!("`{kind}` is not the expected shape: {e}"),
            })?;
        Ok(raw
            .into_iter()
            .map(|v| MetaVersion {
                version: v.version,
                stable: v.stable,
            })
            .collect())
    }

    /// Minecraft versions the loader can run, newest first, snapshots
    /// included (each row carries its own `stable` flag).
    pub fn game_versions(&self) -> Result<Vec<MetaVersion>, CoreError> {
        self.list("game")
    }

    /// Loader versions, newest first.
    pub fn loader_versions(&self) -> Result<Vec<MetaVersion>, CoreError> {
        self.list("loader")
    }

    /// Installer versions, newest first.
    pub fn installer_versions(&self) -> Result<Vec<MetaVersion>, CoreError> {
        self.list("installer")
    }

    /// Resolve the launcher jar for a game version. `loader`/`installer`
    /// are explicit pins (`None` = the newest **stable** upstream
    /// publishes). An explicit pin that upstream does not know is a typed
    /// `Http { status: 404 }` before any download starts — the daemon's
    /// create flow maps that to `CATALOG_NOT_FOUND`, the same
    /// fast-rejection discipline the Fill family follows.
    pub fn resolve_server_jar(
        &self,
        game: &str,
        loader: Option<&str>,
        installer: Option<&str>,
    ) -> Result<FabricServerJar, CoreError> {
        let games = self.game_versions()?;
        if !games.iter().any(|v| v.version == game) {
            return Err(not_found(&format!("Minecraft version {game}")));
        }
        let loaders = self.loader_versions()?;
        let loader = pick(&loaders, loader, "loader")?;
        let installers = self.installer_versions()?;
        let installer = pick(&installers, installer, "installer")?;
        Ok(FabricServerJar {
            name: format!("fabric-server-mc.{game}-loader.{loader}-launcher.{installer}.jar"),
            url: format!(
                "{}/v2/versions/loader/{game}/{loader}/{installer}/server/jar",
                self.base
            ),
            loader,
            installer,
        })
    }
}

/// Resolve a pin or default to the newest stable entry. A pinned version
/// upstream does not know — or a family with nothing stable behind it —
/// is the same typed 404 the Fill family answers for unknown builds.
fn pick(list: &[MetaVersion], pin: Option<&str>, kind: &str) -> Result<String, CoreError> {
    match pin {
        Some(pinned) => list
            .iter()
            .find(|v| v.version == pinned)
            .map(|v| v.version.clone())
            .ok_or_else(|| not_found(&format!("{kind} version {pinned}"))),
        None => list
            .iter()
            .find(|v| v.stable)
            .map(|v| v.version.clone())
            .ok_or_else(|| not_found(&format!("stable {kind} version"))),
    }
}

fn not_found(what: &str) -> CoreError {
    CoreError::Http {
        url: "fabric meta".to_owned(),
        status: 404,
        reason: format!("{what} not found"),
    }
}

// Wire shapes (only the fields this project reads; serde ignores the
// rest — the real entries carry `maven` and `url` fields alongside).
#[derive(Deserialize)]
struct RawMetaVersion {
    version: String,
    #[serde(default)]
    stable: bool,
}
