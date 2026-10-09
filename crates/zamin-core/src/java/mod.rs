//! Java runtime discovery and inspection (ADR-0008 seam drives process
//! spawning; this module decides *what* to run and reads the results).
//!
//! Discovery enumerates candidates; inspection asks the JVM itself via
//! `java -XshowSettings:properties -version` — never directory names.

pub mod fetch;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::CoreError;
use crate::platform::{process, SpawnLimits, SpawnSpec};

pub const INSPECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq)]
pub struct JavaInfo {
    pub path: PathBuf,
    pub major: u32,
    pub version_string: String,
    pub vendor: String,
}

impl JavaInfo {
    /// True when this runtime satisfies a required major version.
    pub fn satisfies(&self, required_major: u32) -> bool {
        self.major >= required_major
    }
}

/// All candidate `java` executables we can find, deduplicated, existing
/// first. Sources: `PATH`, `JAVA_HOME`, and the usual per-OS install roots
/// (the platform seam owns which roots exist).
pub fn candidate_paths() -> Vec<PathBuf> {
    let exe = crate::platform::java_exe_name();
    let mut out = Vec::new();

    if let Some(home) = std::env::var_os("JAVA_HOME") {
        out.push(PathBuf::from(home).join("bin").join(exe));
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            out.push(dir.join(exe));
        }
    }
    for root in crate::platform::java_install_roots() {
        out.push(root.join("bin").join(exe));
    }

    let mut seen = std::collections::BTreeSet::new();
    out.retain(|p| {
        let key = normalize(p);
        seen.insert(key.clone()) && p.is_file()
    });
    out
}

fn normalize(p: &Path) -> String {
    p.to_string_lossy().to_ascii_lowercase().replace('/', "\\")
}

/// `java` executables inside a managed install root (`<data>/java`):
/// one level of runtime directories, each with a `bin` folder — the
/// layout the JDK fetch produces. Candidates are still inspected before
/// use, exactly like PATH discoveries (never trust directory names).
pub fn managed_candidates(root: &Path) -> Vec<PathBuf> {
    let exe = crate::platform::java_exe_name();
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(root) else {
        return out;
    };
    for runtime in read.flatten() {
        let bin = runtime.path().join("bin").join(exe);
        if bin.is_file() {
            out.push(bin);
        }
    }
    let direct = root.join("bin").join(exe);
    if direct.is_file() {
        out.push(direct);
    }
    out.sort();
    out
}

/// Ask the JVM for the truth. Parses stderr of
/// `java -XshowSettings:properties -version`.
pub fn inspect(java_path: &Path) -> Result<JavaInfo, CoreError> {
    let spec = SpawnSpec {
        program: java_path.to_path_buf(),
        args: vec![
            "-XshowSettings:properties".to_owned(),
            "-version".to_owned(),
        ],
        working_dir: std::env::temp_dir(),
        // The inspect probe carries no caps: it is the daemon's own
        // short-lived child, not a server tree.
        limits: SpawnLimits::default(),
    };
    let started = Instant::now();
    let output = process().run_capture(&spec, INSPECT_TIMEOUT).map_err(|e| {
        CoreError::JavaInspectFailed {
            path: java_path.to_path_buf(),
            reason: e.to_string(),
        }
    })?;
    let _ = started.elapsed();

    let mut properties = BTreeMap::new();
    for line in output.stderr.lines().chain(output.stdout.lines()) {
        if let Some((key, value)) = line.trim().split_once(" = ") {
            properties.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    let version_string =
        properties
            .get("java.version")
            .cloned()
            .ok_or_else(|| CoreError::JavaInspectFailed {
                path: java_path.to_path_buf(),
                reason: "no java.version in -XshowSettings output".to_owned(),
            })?;
    let major = parse_major(&version_string).ok_or_else(|| CoreError::JavaInspectFailed {
        path: java_path.to_path_buf(),
        reason: format!("unreadable version string {version_string:?}"),
    })?;
    let vendor = properties.get("java.vendor").cloned().unwrap_or_default();

    Ok(JavaInfo {
        path: java_path.to_path_buf(),
        major,
        version_string,
        vendor,
    })
}

/// "1.8.0_402" → 8; "17.0.11" → 17; "25" → 25.
pub fn parse_major(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// Required Java major for a Minecraft version. Unknown versions get `None`
/// (no constraint); the table is data, extended as MC releases land.
pub fn required_major(mc_version: &str) -> Option<u32> {
    let (maj, min) = split_mc_version(mc_version)?;
    match (maj, min) {
        (m, _) if m > 1 => None,
        (1, n) if n >= 21 => Some(21),
        (1, n) if (17..=20).contains(&n) => Some(17),
        (1, 16) => Some(11),
        _ => Some(8),
    }
}

fn split_mc_version(v: &str) -> Option<(u32, u32)> {
    let mut it = v.split('.');
    let maj: u32 = it.next()?.parse().ok()?;
    let min: u32 = it.next().unwrap_or("0").parse().unwrap_or(0);
    Some((maj, min))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_major_versions() {
        assert_eq!(parse_major("1.8.0_402"), Some(8));
        assert_eq!(parse_major("17.0.11"), Some(17));
        assert_eq!(parse_major("21.0.3"), Some(21));
        assert_eq!(parse_major("25"), Some(25));
        assert_eq!(parse_major("garbage"), None);
    }

    #[test]
    fn required_major_table() {
        assert_eq!(required_major("1.8.9"), Some(8));
        assert_eq!(required_major("1.16.5"), Some(11));
        assert_eq!(required_major("1.17.1"), Some(17));
        assert_eq!(required_major("1.20.4"), Some(17));
        assert_eq!(required_major("1.21.1"), Some(21));
        assert_eq!(required_major("26.2"), None, "unknown future versioning");
    }

    #[test]
    fn satisfies_is_a_floor() {
        let info = JavaInfo {
            path: PathBuf::from("java"),
            major: 21,
            version_string: "21.0.3".into(),
            vendor: "Eclipse Adoptium".into(),
        };
        assert!(info.satisfies(17));
        assert!(info.satisfies(21));
        assert!(!info.satisfies(25));
    }
}
