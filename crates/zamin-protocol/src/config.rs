//! Server configuration over the wire (founder vision §37–39, ADR-0019):
//! the Startup, Network, and Settings surfaces read and write the layered
//! config model (ADR-0007) — global defaults under per-server overrides,
//! with per-field provenance so a client never guesses where a value came
//! from.
//!
//! Patch semantics are tri-state, because "unset this override" must be
//! expressible on the wire: an absent field keeps the current override, a
//! `null` clears the override (the server inherits the global default),
//! a value sets it. `Option<Option<T>>` carries exactly that.
//!
//! Everything here is pure data and codec, like the rest of the crate.

use serde::{Deserialize, Deserializer, Serialize};

/// Tri-state deserialization: a field present in the JSON (whether `null`
/// or a value) becomes `Some(...)`; an absent field stays `None`. Without
/// this, serde collapses a JSON `null` into the same `None` as absence
/// and "clear this override" would be indistinguishable from "don't
/// touch it".
fn tri_state<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

/// Where an effective value came from (ADR-0007's provenance, on the
/// wire): the global defaults file, or this server's own config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldProvenance {
    Global,
    Custom,
}

/// The effective (layered) settings as `config.get` answers them. This is
/// what the server actually runs with — not the raw files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveSettingsView {
    pub stop_timeout_secs: u32,
    pub startup_timeout_secs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_memory_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u32>,
    /// Always serialized, even empty: a list field is always a list on
    /// the wire. Omitting it made older clients crash reading `.join`
    /// off `undefined` — the Startup tab's whole-tab crash (P0, fixed).
    #[serde(default)]
    pub extra_jvm_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_major_required: Option<u32>,
    pub backup_keep: u32,
}

/// Per-field provenance, field-for-field with `EffectiveSettingsView`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceView {
    pub stop_timeout_secs: FieldProvenance,
    pub startup_timeout_secs: FieldProvenance,
    pub port: FieldProvenance,
    pub min_memory_mb: FieldProvenance,
    pub max_memory_mb: FieldProvenance,
    pub extra_jvm_args: FieldProvenance,
    pub java_path: FieldProvenance,
    pub mc_version: FieldProvenance,
    pub java_major_required: FieldProvenance,
    pub backup_keep: FieldProvenance,
}

/// `config.get {serverId}` — the §38/§39 form: the effective view plus the
/// provenance map, plus the two fields the per-server file owns that are
/// not layered settings (`displayName` lives in the registry, `jar` in the
/// per-server file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigGetParams {
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigGetResult {
    pub server_id: String,
    pub display_name: String,
    /// Server-root-relative jar path; `None` means the built-in default
    /// (`server.jar`) applies — the same rule the spawner follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jar: Option<String>,
    pub effective: EffectiveSettingsView,
    pub provenance: ProvenanceView,
}

/// One layered setting in a `config.set` patch. Absent = keep whatever is
/// there now; `null` = clear this server's override (inherit the global
/// default); a value = set the override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerSettingsPatch {
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub stop_timeout_secs: Option<Option<u32>>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub startup_timeout_secs: Option<Option<u32>>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub port: Option<Option<u16>>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub min_memory_mb: Option<Option<u32>>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_memory_mb: Option<Option<u32>>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub extra_jvm_args: Option<Option<Vec<String>>>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub java_path: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub mc_version: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub java_major_required: Option<Option<u32>>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub backup_keep: Option<Option<u32>>,
}

impl ServerSettingsPatch {
    /// True when the patch would change nothing at all.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// `config.set {serverId, displayName?, jar?, settings?}` — a tri-state
/// patch (see the module doc). `displayName` has no "clear" state: a
/// server always has a name. An empty patch (nothing specified) is a
/// typed no-op refusal, not a silent success.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSetParams {
    pub server_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(
        default,
        deserialize_with = "tri_state",
        skip_serializing_if = "Option::is_none"
    )]
    pub jar: Option<Option<String>>,
    #[serde(default)]
    pub settings: ServerSettingsPatch,
}

/// `config.set` answers with the same shape as `config.get` — the fresh
/// effective view, so a client never has to re-read after a write.
pub type ConfigSetResult = ConfigGetResult;

/// `network.status {serverId}` (founder §37): what port the server wants,
/// what port its `server.properties` actually names, whether that port
/// can be bound right now, and which other managed servers claim the same
/// desired port. Availability is a bind-test — the final authority is the
/// server binding at boot; this is the "detect conflicts before startup
/// where possible" read, not a guarantee.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatusParams {
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatusResult {
    pub server_id: String,
    /// The layered desired port (config model); `None` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desired_port: Option<u16>,
    /// The port `server.properties` names (the boot authority). A server
    /// directory without the file yet answers `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties_port: Option<u16>,
    /// `server.properties`'s `server-ip`; `None` when the file is absent,
    /// an empty string means all interfaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,
    /// Bind-test of `desired_port.or(properties_port)` at answer time.
    /// `None` when neither port is known — there is nothing to probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_available: Option<bool>,
    /// Other registered servers whose desired port equals this server's
    /// desired port. Self never appears here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_state_patch_absent_null_and_value() {
        // Absent → keep; null → clear; value → set. The double Option is
        // the whole point; serde must not collapse it.
        let patch: ServerSettingsPatch = serde_json::from_value(serde_json::json!({
            "port": 25566,
            "maxMemoryMb": null
        }))
        .unwrap();
        assert_eq!(patch.port, Some(Some(25566)));
        assert_eq!(patch.max_memory_mb, Some(None));
        assert_eq!(patch.stop_timeout_secs, None, "absent stays keep");
        assert!(!patch.is_empty());

        let empty: ServerSettingsPatch = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn tri_state_patch_serializes_null_not_absent() {
        let patch = ServerSettingsPatch {
            min_memory_mb: Some(None),
            port: Some(Some(25565)),
            ..ServerSettingsPatch::default()
        };
        let value = serde_json::to_value(&patch).unwrap();
        assert_eq!(value["minMemoryMb"], serde_json::Value::Null);
        assert_eq!(value["port"], 25565);
        assert!(value.get("stopTimeoutSecs").is_none());
    }

    #[test]
    fn provenance_reads_back_kebab() {
        let prov: ProvenanceView = serde_json::from_value(serde_json::json!({
            "stopTimeoutSecs": "global",
            "startupTimeoutSecs": "global",
            "port": "custom",
            "minMemoryMb": "global",
            "maxMemoryMb": "custom",
            "extraJvmArgs": "global",
            "javaPath": "global",
            "mcVersion": "global",
            "javaMajorRequired": "global",
            "backupKeep": "global"
        }))
        .unwrap();
        assert_eq!(prov.port, FieldProvenance::Custom);
        assert_eq!(prov.max_memory_mb, FieldProvenance::Custom);
        assert_eq!(prov.stop_timeout_secs, FieldProvenance::Global);
    }
}
