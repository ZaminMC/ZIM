//! Server discovery (founder §64): what servers does this machine already
//! have? The daemon answers from two honest sources — the registry (what
//! ZaminPanel manages) and a bounded scan of operator-configured roots for
//! **server directories** (a `server.properties` on disk) and **supported
//! server JARs** (filename classification, stated as evidence, never
//! guessed as support). Everything rides ADR-0009's rules: symlinks are
//! never followed, budgets bound the walk, and a directory that vanishes
//! mid-scan shrinks the answer instead of failing it.

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::fsops::STAGING_DIR_NAME;
use crate::server::marker;
use crate::server::ServerId;

/// How deep the scan descends: the root itself, its directories, and one
/// more level — `~/mc/servers/<name>/` is the deep layout discovery is for;
/// a server's own `world/` and `plugins/` trees are not discovery targets.
pub const SCAN_MAX_DEPTH: usize = 2;

/// The entries budget for the whole scan. Discovery answers "what is here",
/// not "walk everything" — past the budget the result says `truncated` and
/// stops, the same honesty `files.search` keeps.
pub const SCAN_ENTRY_BUDGET: u64 = 4096;

/// One found thing. `Directory` is a directory that looks like a server
/// (a `server.properties` inside); `Jar` is a supported server jar standing
/// on its own (no `server.properties` beside it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub path: PathBuf,
    pub kind: DiscoveredKind,
    /// A marker-bound server id, when the directory is (or was) managed.
    pub marker: Option<ServerId>,
    /// From `server.properties`, for directories.
    pub port: Option<u16>,
    /// The jar family the filename classified as (`paper`, `fabric`, …),
    /// stated as the evidence it is — a filename match, not a probe.
    pub platform: Option<&'static str>,
    /// The jar filename that produced `platform`, when one was found.
    pub jar_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveredKind {
    Directory,
    Jar,
}

impl DiscoveredKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DiscoveredKind::Directory => "directory",
            DiscoveredKind::Jar => "jar",
        }
    }
}

/// The scan outcome: found candidates plus the honesty fields — skipped
/// roots when the root could not be read at all (absent, or the OS
/// refused), `truncated`/`scanned` carrying the budget truth.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub found: Vec<Discovered>,
    pub scanned: u64,
    pub truncated: bool,
    pub skipped_roots: Vec<PathBuf>,
}

/// Classify a jar filename into a server family. The match is on the
/// filename alone and says exactly that much: a file NAMED like a Paper
/// build is a paper-class candidate — not a verified server. Installer,
/// client, sources and launcher jars are deliberately not candidates.
pub fn classify_jar(file_name: &str) -> Option<&'static str> {
    let lower = file_name.to_lowercase();
    let stem = lower.strip_suffix(".jar")?;
    // An installer / client / sources artifact is not a runnable server —
    // naming it one would make discovery lie.
    for poison in [
        "installer",
        "sources",
        "javadoc",
        "decomp",
        "client",
        "launcher",
        "shim",
    ] {
        if stem.contains(poison) {
            return None;
        }
    }
    const FAMILIES: &[(&str, &str)] = &[
        ("minecraft_server", "vanilla"),
        ("paper", "paper"),
        ("purpur", "purpur"),
        ("folia", "folia"),
        ("spigot", "spigot"),
        ("fabric", "fabric"),
        ("neoforge", "neoforge"),
        ("forge", "forge"),
        ("velocity", "velocity"),
        ("waterfall", "waterfall"),
        ("mohist", "mohist"),
        ("arclight", "arclight"),
        ("quilt", "quilt"),
        ("spongevanilla", "sponge"),
        ("spongeforge", "sponge"),
        ("sponge", "sponge"),
    ];
    if stem == "server" {
        // The plain `server.jar` convention: a vanilla-family runtime.
        return Some("vanilla");
    }
    FAMILIES
        .iter()
        .find(|(needle, _)| stem.starts_with(needle))
        .map(|(_, family)| *family)
}

/// `server-port=` from server.properties bytes. Properties are latin-1
/// key=value lines; a lossy read is honest for a number field, and an
/// absent or malformed port is `None` — discovery reports what it read.
pub fn read_port(properties: &[u8]) -> Option<u16> {
    let text = String::from_utf8_lossy(properties);
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("server-port=") {
            return value.trim().parse::<u16>().ok();
        }
    }
    None
}

/// Scan operator roots for server directories and supported jars.
///
/// Per root, per level: a directory containing `server.properties` is a
/// candidate and is **not** descended into (its `world/` and `plugins/`
/// trees are the server's own business); otherwise supported jars at that
/// level are candidates and real subdirectories are queued — symlinks,
/// hidden entries and the staging area never are. The walk is bounded by
/// depth and entry budget; the report carries both truths.
pub fn scan_roots(roots: &[PathBuf]) -> ScanReport {
    let mut report = ScanReport::default();
    let mut budget = SCAN_ENTRY_BUDGET;
    for root in roots {
        if report.truncated {
            break;
        }
        let canonical = match root.canonicalize() {
            Ok(path) => path,
            // An absent or unreadable root is named, not faked — the
            // operator configured it, so its absence is their information.
            Err(_) => {
                report.skipped_roots.push(root.clone());
                continue;
            }
        };
        scan_dir(&canonical, 0, &mut report, &mut budget);
    }
    // One directory can sit under two configured roots; it is still one
    // server. Dedupe by path — the first sighting wins, and the sort
    // makes the order deterministic.
    report.found.sort_by(|a, b| a.path.cmp(&b.path));
    report.found.dedup_by(|a, b| a.path == b.path);
    report
}

fn scan_dir(dir: &Path, depth: usize, report: &mut ScanReport, budget: &mut u64) {
    if depth > SCAN_MAX_DEPTH {
        report.truncated = true;
        return;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(_) => return,
    };
    // One pass over the level, deterministic order regardless of the OS's
    // readdir mood. Every accepted entry costs budget — including the ones
    // skipped by rule, which are still work the disk did.
    let mut has_properties = false;
    let mut port: Option<u16> = None;
    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut jars: Vec<(String, &'static str)> = Vec::new();
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *budget == 0 {
            report.truncated = true;
            return;
        }
        *budget -= 1;
        report.scanned += 1;
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            // ADR-0009: never followed, never dereferenced, never a hit.
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            if name.starts_with('.') || name == STAGING_DIR_NAME {
                continue;
            }
            subdirs.push(entry.path());
            continue;
        }
        if name == "server.properties" {
            has_properties = true;
            if port.is_none() {
                let bytes = std::fs::read(entry.path()).unwrap_or_default();
                port = read_port(&bytes);
            }
            continue;
        }
        if let Some(family) = classify_jar(&name) {
            jars.push((name, family));
        }
    }
    if has_properties {
        // A managed-shaped directory: ask the marker who it is. A corrupt
        // marker reads as None here — the daemon's merge decides what that
        // means against the registry; discovery reports the shape.
        let marker_id = marker::read_marker(dir).unwrap_or(None);
        report.found.push(Discovered {
            path: dir.to_path_buf(),
            kind: DiscoveredKind::Directory,
            marker: marker_id,
            port,
            platform: jars.first().map(|(_, family)| *family),
            jar_name: jars.first().map(|(name, _)| name.clone()),
        });
        // Not descended into: a server directory's innards are its own.
        return;
    }
    for (name, family) in jars {
        report.found.push(Discovered {
            path: dir.join(&name),
            kind: DiscoveredKind::Jar,
            marker: None,
            port: None,
            platform: Some(family),
            jar_name: Some(name),
        });
    }
    for sub in subdirs {
        scan_dir(&sub, depth + 1, report, budget);
    }
}
