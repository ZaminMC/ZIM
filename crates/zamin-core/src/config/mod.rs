//! Configuration model (ADR-0007): TOML for human-edited files, JSON for
//! machine state; global defaults layered under per-server settings with
//! per-field provenance; versioned with migrators.
//!
//! `server.properties` is owned by Minecraft and is *not* part of this
//! model — the supervisor reconciles it explicitly, never silently.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::fsops::atomic_write;

pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Defaults every server inherits unless it overrides the field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerSettingsDefaults {
    pub stop_timeout_secs: Option<u32>,
    pub startup_timeout_secs: Option<u32>,
    pub port: Option<u16>,
    pub min_memory_mb: Option<u32>,
    pub max_memory_mb: Option<u32>,
    pub extra_jvm_args: Option<Vec<String>>,
    pub java_path: Option<PathBuf>,
    /// The Minecraft version this server runs (e.g. "1.21.1"). Drives the
    /// derived Java-major requirement when `javaMajorRequired` is absent.
    pub mc_version: Option<String>,
    /// Direct override of the required Java major version; wins over the
    /// value derived from `mcVersion` (ADR-0005 preflight).
    pub java_major_required: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalConfigFile {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub defaults: ServerSettingsDefaults,
}

impl Default for GlobalConfigFile {
    fn default() -> Self {
        GlobalConfigFile {
            schema_version: CONFIG_SCHEMA_VERSION,
            defaults: ServerSettingsDefaults {
                stop_timeout_secs: Some(60),
                startup_timeout_secs: Some(120),
                ..ServerSettingsDefaults::default()
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerConfigFile {
    #[serde(rename = "schemaVersion", default = "default_schema_version")]
    pub schema_version: u32,
    pub display_name: Option<String>,
    /// Server-root-relative jar path, e.g. `server.jar`.
    pub jar: Option<String>,
    pub settings: ServerSettingsDefaults,
}

fn default_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    Global,
    Custom,
}

/// Effective settings after layering, with provenance per field so the UI
/// never has to guess where a value came from.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveSettings {
    pub stop_timeout_secs: u32,
    pub startup_timeout_secs: u32,
    pub port: Option<u16>,
    pub min_memory_mb: Option<u32>,
    pub max_memory_mb: Option<u32>,
    pub extra_jvm_args: Vec<String>,
    pub java_path: Option<PathBuf>,
    pub mc_version: Option<String>,
    pub java_major_required: Option<u32>,
}

impl EffectiveSettings {
    /// Provenance per field, in field order above — the UI's "Using global
    /// default" vs "Custom value" signal (ADR-0007).
    pub fn provenance(
        global: &ServerSettingsDefaults,
        per_server: &ServerSettingsDefaults,
    ) -> [Provenance; 9] {
        [
            field_provenance(global.stop_timeout_secs, per_server.stop_timeout_secs),
            field_provenance(global.startup_timeout_secs, per_server.startup_timeout_secs),
            field_provenance(global.port, per_server.port),
            field_provenance(global.min_memory_mb, per_server.min_memory_mb),
            field_provenance(global.max_memory_mb, per_server.max_memory_mb),
            field_provenance(
                global.extra_jvm_args.clone(),
                per_server.extra_jvm_args.clone(),
            ),
            field_provenance(global.java_path.clone(), per_server.java_path.clone()),
            field_provenance(global.mc_version.clone(), per_server.mc_version.clone()),
            field_provenance(global.java_major_required, per_server.java_major_required),
        ]
    }
}

fn field_provenance<T: PartialEq>(global: Option<T>, per_server: Option<T>) -> Provenance {
    if per_server.is_some() {
        Provenance::Custom
    } else if global.is_some() {
        Provenance::Global
    } else {
        // Neither file sets it; the caller applies the built-in default.
        Provenance::Global
    }
}

/// Layer the two files. Absent per-server fields inherit globals; absent
/// globals fall back to built-in defaults.
pub fn layer(
    global: &ServerSettingsDefaults,
    per_server: &ServerSettingsDefaults,
) -> EffectiveSettings {
    let pick = |g: Option<u32>, p: Option<u32>, builtin: u32| p.or(g).unwrap_or(builtin);
    EffectiveSettings {
        stop_timeout_secs: pick(global.stop_timeout_secs, per_server.stop_timeout_secs, 60),
        startup_timeout_secs: pick(
            global.startup_timeout_secs,
            per_server.startup_timeout_secs,
            120,
        ),
        port: per_server.port.or(global.port),
        min_memory_mb: per_server.min_memory_mb.or(global.min_memory_mb),
        max_memory_mb: per_server.max_memory_mb.or(global.max_memory_mb),
        extra_jvm_args: per_server
            .extra_jvm_args
            .clone()
            .or_else(|| global.extra_jvm_args.clone())
            .unwrap_or_default(),
        java_path: per_server
            .java_path
            .clone()
            .or_else(|| global.java_path.clone()),
        mc_version: per_server
            .mc_version
            .clone()
            .or_else(|| global.mc_version.clone()),
        java_major_required: per_server
            .java_major_required
            .or(global.java_major_required),
    }
}

pub fn load_global(path: &Path) -> Result<GlobalConfigFile, CoreError> {
    if !path.exists() {
        return Ok(GlobalConfigFile::default());
    }
    let raw = std::fs::read_to_string(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let file: GlobalConfigFile = toml::from_str(&raw).map_err(|e| CoreError::RegistryCorrupt {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    check_version(&file.schema_version, path)?;
    Ok(file)
}

pub fn save_global(path: &Path, file: &GlobalConfigFile) -> Result<(), CoreError> {
    let raw = toml::to_string_pretty(file).map_err(|e| CoreError::RegistryCorrupt {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    atomic_write(path, raw.as_bytes())
}

pub fn load_server(path: &Path) -> Result<ServerConfigFile, CoreError> {
    if !path.exists() {
        return Ok(ServerConfigFile {
            schema_version: CONFIG_SCHEMA_VERSION,
            ..ServerConfigFile::default()
        });
    }
    let raw = std::fs::read_to_string(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let file: ServerConfigFile = toml::from_str(&raw).map_err(|e| CoreError::RegistryCorrupt {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    check_version(&file.schema_version, path)?;
    Ok(file)
}

pub fn save_server(path: &Path, file: &ServerConfigFile) -> Result<(), CoreError> {
    let mut file = file.clone();
    file.schema_version = CONFIG_SCHEMA_VERSION;
    let raw = toml::to_string_pretty(&file).map_err(|e| CoreError::RegistryCorrupt {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    atomic_write(path, raw.as_bytes())
}

fn check_version(found: &u32, path: &Path) -> Result<(), CoreError> {
    if *found != CONFIG_SCHEMA_VERSION {
        return Err(CoreError::SchemaVersion {
            path: path.to_path_buf(),
            found: *found,
            expected: CONFIG_SCHEMA_VERSION,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::registry::tempdir;

    #[test]
    fn layering_prefers_custom_then_global_then_builtin() {
        let global = ServerSettingsDefaults {
            stop_timeout_secs: Some(45),
            port: Some(25565),
            ..Default::default()
        };
        let per = ServerSettingsDefaults {
            stop_timeout_secs: Some(30),
            max_memory_mb: Some(8192),
            ..Default::default()
        };
        let effective = layer(&global, &per);
        assert_eq!(effective.stop_timeout_secs, 30, "custom wins");
        assert_eq!(effective.port, Some(25565), "global inherits");
        assert_eq!(effective.max_memory_mb, Some(8192));
        assert_eq!(effective.startup_timeout_secs, 120, "builtin fallback");
        assert_eq!(effective.extra_jvm_args, Vec::<String>::new());
    }

    #[test]
    fn provenance_marks_custom_fields() {
        let per = ServerSettingsDefaults {
            stop_timeout_secs: Some(30),
            ..Default::default()
        };
        let prov = EffectiveSettings::provenance(&ServerSettingsDefaults::default(), &per);
        assert_eq!(prov[0], Provenance::Custom, "stop timeout is custom");
        assert_eq!(prov[1], Provenance::Global, "startup timeout is global");
    }

    #[test]
    fn java_requirement_layers_and_prefers_direct_override() {
        // Derived from mcVersion when no direct override exists.
        let global = ServerSettingsDefaults {
            mc_version: Some("1.21.1".to_owned()),
            ..Default::default()
        };
        let effective = layer(&global, &ServerSettingsDefaults::default());
        assert_eq!(effective.mc_version.as_deref(), Some("1.21.1"));
        assert_eq!(effective.java_major_required, None);

        // The direct override wins over a derived value.
        let per = ServerSettingsDefaults {
            java_major_required: Some(25),
            ..Default::default()
        };
        let effective = layer(&global, &per);
        assert_eq!(effective.java_major_required, Some(25));
        assert_eq!(
            effective.mc_version.as_deref(),
            Some("1.21.1"),
            "mcVersion still visible for the UI"
        );
    }

    #[test]
    fn global_config_round_trips_through_toml() {
        let dir = tempdir::scoped("config");
        let path = dir.path.join("config.toml");
        let file = GlobalConfigFile::default();
        save_global(&path, &file).unwrap();
        let loaded = load_global(&path).unwrap();
        assert_eq!(loaded, file);
    }

    #[test]
    fn wrong_version_is_rejected() {
        let dir = tempdir::scoped("config-version");
        let path = dir.path.join("config.toml");
        std::fs::write(&path, "schemaVersion = 42\n\n[defaults]\n").unwrap();
        assert!(matches!(
            load_global(&path),
            Err(CoreError::SchemaVersion { .. })
        ));
    }
}
