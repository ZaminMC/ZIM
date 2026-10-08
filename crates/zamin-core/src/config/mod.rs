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
    /// Retention for the server's backups: keep the newest N after every
    /// successful backup (ARCH-REVIEW §16.5 — retention is required, not
    /// optional). The built-in default is applied by `layer`.
    pub backup_keep: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalConfigFile {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub defaults: ServerSettingsDefaults,
    /// Discovery's operator-configured scan roots (§64): where the daemon
    /// looks for unregistered servers. Absent in older files → serde's
    /// default (no roots); the instance dir is always scanned implicitly.
    #[serde(default)]
    pub discovery: DiscoveryConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscoveryConfig {
    /// Absolute paths the discovery scan walks (two levels deep, ADR-0009
    /// rules). Managed instance dirs live outside this list — the daemon
    /// scans its own instances dir regardless.
    pub roots: Vec<String>,
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
            discovery: DiscoveryConfig::default(),
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
    pub backup_keep: u32,
}

/// The built-in default retention when neither config file sets it.
pub const BACKUP_KEEP_DEFAULT: u32 = 10;

impl EffectiveSettings {
    /// Provenance per field, in field order above — the UI's "Using global
    /// default" vs "Custom value" signal (ADR-0007).
    pub fn provenance(
        global: &ServerSettingsDefaults,
        per_server: &ServerSettingsDefaults,
    ) -> [Provenance; 10] {
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
            field_provenance(global.backup_keep, per_server.backup_keep),
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
        backup_keep: per_server
            .backup_keep
            .or(global.backup_keep)
            .unwrap_or(BACKUP_KEEP_DEFAULT),
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

/// Sanity bounds for the layered settings a client may write (ADR-0019).
/// The spawner will obey whatever lands in the files, so nonsense is
/// refused at the write, with the field named — not discovered at the
/// next boot as a baffling preflight failure.
pub const PORT_MIN: u16 = 1024;
pub const PORT_MAX: u16 = u16::MAX - 1; // 65535 is legal but reserved for ephemeral ranges on common OSes
pub const MEMORY_MIN_MB: u32 = 16;
pub const MEMORY_MAX_MB: u32 = 1_048_576; // 1 TiB
pub const TIMEOUT_MAX_SECS: u32 = 86_400; // a day; anything longer is a mistake
pub const BACKUP_KEEP_MAX: u32 = 1_000;
pub const JAVA_MAJOR_MIN: u32 = 8;
pub const JAVA_MAJOR_MAX: u32 = 100;

/// Validate one override set before it is written. Every rule states the
/// field it names, so the error can travel to the UI verbatim.
pub fn validate_field(field: &str, value: Option<u32>) -> Result<(), CoreError> {
    let Some(value) = value else {
        return Ok(()); // clearing an override is always legal
    };
    let check = |ok: bool, reason: &str| -> Result<(), CoreError> {
        if ok {
            Ok(())
        } else {
            Err(CoreError::ConfigInvalid {
                field: field.to_owned(),
                reason: reason.to_owned(),
            })
        }
    };
    match field {
        "port" => {
            let port = u16::try_from(value).map_err(|_| CoreError::ConfigInvalid {
                field: field.to_owned(),
                reason: "port must fit in 16 bits".to_owned(),
            })?;
            check(
                (PORT_MIN..=PORT_MAX).contains(&port),
                &format!("the port must be between {PORT_MIN} and {PORT_MAX}"),
            )
        }
        "minMemoryMb" | "maxMemoryMb" => check(
            (MEMORY_MIN_MB..=MEMORY_MAX_MB).contains(&value),
            &format!("memory must be between {MEMORY_MIN_MB} and {MEMORY_MAX_MB} MiB"),
        ),
        "stopTimeoutSecs" | "startupTimeoutSecs" => check(
            (1..=TIMEOUT_MAX_SECS).contains(&value),
            &format!("the timeout must be between 1 and {TIMEOUT_MAX_SECS} seconds"),
        ),
        "backupKeep" => check(
            (1..=BACKUP_KEEP_MAX).contains(&value),
            &format!("backup retention must be between 1 and {BACKUP_KEEP_MAX}"),
        ),
        "javaMajorRequired" => check(
            (JAVA_MAJOR_MIN..=JAVA_MAJOR_MAX).contains(&value),
            &format!("the Java major must be between {JAVA_MAJOR_MIN} and {JAVA_MAJOR_MAX}"),
        ),
        _ => Ok(()), // unknown fields are the caller's business
    }
}

/// Validate the min/max pairing that only makes sense together.
pub fn validate_memory_pair(
    min_memory_mb: Option<u32>,
    max_memory_mb: Option<u32>,
) -> Result<(), CoreError> {
    if let (Some(min), Some(max)) = (min_memory_mb, max_memory_mb) {
        if min > max {
            return Err(CoreError::ConfigInvalid {
                field: "minMemoryMb".to_owned(),
                reason: format!("the minimum ({min} MiB) must not exceed the maximum ({max} MiB)"),
            });
        }
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

    #[test]
    fn field_validation_names_the_field() {
        // Port bounds.
        assert!(validate_field("port", Some(25_565)).is_ok());
        assert!(validate_field("port", Some(80)).is_err());
        assert!(validate_field("port", Some(0)).is_err());
        // Memory bounds and the pairing.
        assert!(validate_field("minMemoryMb", Some(512)).is_ok());
        assert!(validate_field("maxMemoryMb", Some(8)).is_err());
        assert!(validate_memory_pair(Some(512), Some(1024)).is_ok());
        assert!(validate_memory_pair(Some(2048), Some(1024)).is_err());
        // Timeouts, retention, java major.
        assert!(validate_field("stopTimeoutSecs", Some(60)).is_ok());
        assert!(validate_field("stopTimeoutSecs", Some(0)).is_err());
        assert!(validate_field("startupTimeoutSecs", Some(86_401)).is_err());
        assert!(validate_field("backupKeep", Some(0)).is_err());
        assert!(validate_field("javaMajorRequired", Some(21)).is_ok());
        assert!(validate_field("javaMajorRequired", Some(4)).is_err());
        // Clearing is always legal; unknown fields are the caller's.
        assert!(validate_field("port", None).is_ok());
        assert!(validate_field("somethingElse", Some(1)).is_ok());

        let err = validate_field("port", Some(80)).unwrap_err();
        assert!(
            err.to_string().contains("port"),
            "the field is named: {err}"
        );
    }
}
