//! Discovery's unit proofs: filename classification is honest (a family
//! only where the name says so, never an installer), the scan finds
//! directories and jars at every documented depth, symlinks and hidden and
//! staging entries never appear, a server directory is not descended into,
//! the marker is read when present, and the budget tells the truth when
//! the walk is cut short.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use super::{classify_jar, read_port, scan_roots, DiscoveredKind, SCAN_ENTRY_BUDGET};
use crate::server::marker::write_marker;
use crate::server::registry::tempdir;
use crate::server::ServerId;

fn dir_with_properties(root: &PathBuf, rel: &str, port: Option<u16>) -> PathBuf {
    let dir = root.join(rel);
    fs::create_dir_all(&dir).unwrap();
    let mut properties = String::from("motd=hello\n");
    if let Some(port) = port {
        properties.push_str(&format!("server-port={port}\n"));
    }
    fs::write(dir.join("server.properties"), properties).unwrap();
    dir
}

#[test]
fn jar_classification_names_the_family_only_when_the_name_says_so() {
    assert_eq!(classify_jar("paper-1.21.4-133.jar"), Some("paper"));
    assert_eq!(classify_jar("PURPUR-1.21.jar"), Some("purpur"));
    assert_eq!(
        classify_jar("fabric-server-mc.1.21.4-loader.0.16.9.jar"),
        Some("fabric")
    );
    assert_eq!(classify_jar("neoforge-21.4.30.jar"), Some("neoforge"));
    assert_eq!(classify_jar("forge-1.21.4-54.0.24-universal.jar"), Some("forge"));
    assert_eq!(classify_jar("server.jar"), Some("vanilla"));
    assert_eq!(classify_jar("minecraft_server.1.21.4.jar"), Some("vanilla"));
    assert_eq!(classify_jar("velocity-3.4.0.jar"), Some("velocity"));
    // Unknown and non-server names say nothing — discovery does not guess.
    assert_eq!(classify_jar("essentialsx-2.20.0.jar"), None);
    assert_eq!(classify_jar("world.zip"), None);
    assert_eq!(classify_jar("notes.txt"), None);
    // An installer is not a runnable server.
    assert_eq!(classify_jar("forge-1.21.4-installer.jar"), None);
    assert_eq!(classify_jar("paper-client.jar"), None);
    assert_eq!(classify_jar("fabric-sources.jar"), None);
}

#[test]
fn port_reading_takes_the_properties_line_and_nothing_else() {
    assert_eq!(read_port(b"motd=hi\nserver-port=25565\n"), Some(25565));
    assert_eq!(read_port(b"server-port= 25577 \n"), Some(25577));
    assert_eq!(read_port(b"motd=no port here\n"), None);
    assert_eq!(read_port(b"server-port=not-a-port\n"), None);
    assert_eq!(read_port(b""), None);
}

#[test]
fn scan_finds_directories_and_jars_at_every_documented_depth() {
    let temp = tempdir::scoped("discovery-depth");
    let root = temp.path.clone();
    // Depth 1 and 2 inside a container root — the layout discovery is for.
    dir_with_properties(&root, "survival", Some(25566));
    dir_with_properties(&root, "group/creative", Some(25567));
    // A jar standing on its own at depth 1 — and one that says nothing.
    fs::create_dir_all(root.join("downloads")).unwrap();
    fs::write(root.join("downloads/paper-1.21.4.jar"), b"jar").unwrap();
    fs::write(root.join("downloads/notes.jar"), b"jar").unwrap();
    // Depth 0: a root that IS a server directory itself.
    let direct = dir_with_properties(&root, "direct-root", Some(25565));

    let report = scan_roots(&[root, direct]);
    assert!(!report.truncated);
    assert!(report.skipped_roots.is_empty());

    let find = |suffix: &str| {
        report
            .found
            .iter()
            .find(|f| f.path.ends_with(suffix))
            .unwrap_or_else(|| panic!("missing {suffix} in {:?}", report.found))
    };
    let direct_hit = find("direct-root");
    assert_eq!(direct_hit.kind, DiscoveredKind::Directory);
    assert_eq!(direct_hit.port, Some(25565));
    // The container root was not a server, so it was walked — and the
    // server directory inside it is a candidate, not its sibling jars.
    let survival = find("survival");
    assert_eq!(survival.port, Some(25566));
    assert_eq!(survival.marker, None);
    let creative = find("creative");
    assert_eq!(creative.port, Some(25567));
    let paper = find("paper-1.21.4.jar");
    assert_eq!(paper.kind, DiscoveredKind::Jar);
    assert_eq!(paper.platform, Some("paper"));
    assert_eq!(paper.jar_name.as_deref(), Some("paper-1.21.4.jar"));
    // The unclassified jar is nowhere, and nothing else inflated the count.
    assert!(!report.found.iter().any(|f| f.path.ends_with("notes.jar")));
    assert_eq!(report.found.len(), 4);
}

#[test]
fn server_directory_is_not_descended_into_and_its_jar_names_its_platform() {
    let temp = tempdir::scoped("discovery-noscroll");
    let root = temp.path.clone();
    let server = dir_with_properties(&root, "main", Some(25565));
    fs::write(server.join("paper-1.21.4.jar"), b"jar").unwrap();
    // A deeper jar inside the server directory must NOT be found: the
    // directory candidate ends the walk into its own innards.
    fs::create_dir_all(server.join("plugins")).unwrap();
    fs::write(server.join("plugins/fabric-server.jar"), b"jar").unwrap();
    // The world tree is never walked either way.
    fs::create_dir_all(server.join("world/region")).unwrap();
    fs::write(server.join("world/region/r.0.0.mca"), b"data").unwrap();

    let report = scan_roots(&[root]);
    assert_eq!(report.found.len(), 1);
    let hit = &report.found[0];
    assert_eq!(hit.kind, DiscoveredKind::Directory);
    assert_eq!(hit.platform, Some("paper"));
    assert_eq!(hit.jar_name.as_deref(), Some("paper-1.21.4.jar"));
    assert!(!report.found.iter().any(|f| f.path.ends_with("fabric-server.jar")));
}

#[test]
fn marker_is_read_and_travels_with_the_directory() {
    let temp = tempdir::scoped("discovery-marker");
    let root = temp.path.clone();
    let server = dir_with_properties(&root, "legacy", None);
    let id = ServerId::parse("legacy").unwrap();
    write_marker(&server, &id).unwrap();

    let report = scan_roots(&[root]);
    assert_eq!(report.found.len(), 1);
    assert_eq!(
        report.found[0].marker.as_ref().map(|m| m.as_str()),
        Some("legacy")
    );
    assert_eq!(report.found[0].port, None);
}

#[test]
fn symlinks_hidden_and_staging_entries_never_appear() {
    let temp = tempdir::scoped("discovery-edges");
    let root = temp.path.clone();
    dir_with_properties(&root, ".hidden-server", Some(1));
    dir_with_properties(&root, ".zamin-staging/build", Some(2));
    dir_with_properties(&root, "real", Some(3));
    // A hidden directory holding a jar.
    fs::create_dir_all(root.join(".stash")).unwrap();
    fs::write(root.join(".stash/paper-1.21.jar"), b"jar").unwrap();
    #[cfg(unix)]
    {
        // A symlinked server directory: never followed, never a hit.
        std::os::unix::fs::symlink(root.join("real"), root.join("alias")).unwrap();
    }

    let report = scan_roots(&[root]);
    let names: Vec<String> = report
        .found
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["real".to_string()]);
}

#[test]
fn missing_root_is_named_and_other_roots_still_scan() {
    let temp = tempdir::scoped("discovery-skip");
    let root = temp.path.clone();
    dir_with_properties(&root, "alive", None);
    let absent = root.join("does-not-exist");

    let report = scan_roots(&[absent.clone(), root]);
    assert_eq!(report.skipped_roots, vec![absent]);
    assert_eq!(report.found.len(), 1);
}

#[test]
fn the_budget_cuts_the_walk_and_says_so() {
    // More entries than the budget allows: directories past the budget are
    // not scanned and `truncated` is the report's word for it.
    let temp = tempdir::scoped("discovery-budget");
    let root = temp.path.clone();
    let count = u32::try_from(SCAN_ENTRY_BUDGET).unwrap() + 10;
    for i in 0..count {
        fs::create_dir_all(root.join(format!("srv{i}"))).unwrap();
        fs::write(
            root.join(format!("srv{i}/server.properties")),
            format!("server-port={}\n", 30_000 + i % 10_000),
        )
        .unwrap();
    }
    let report = scan_roots(&[root]);
    assert!(report.truncated);
    assert!(report.found.len() < count as usize);
    // Every candidate is still honest: a directory with its port read.
    assert!(
        report.found.iter().all(|f| f.kind == DiscoveredKind::Directory),
        "past the budget no half-read candidate may appear"
    );
}

#[test]
fn repeated_scans_are_deterministic() {
    let temp = tempdir::scoped("discovery-stable");
    let root = temp.path.clone();
    dir_with_properties(&root, "b-server", None);
    dir_with_properties(&root, "a-server", None);
    fs::write(root.join("z-paper.jar"), b"jar").unwrap();

    let first = scan_roots(&[root.clone()]);
    let second = scan_roots(&[root]);
    assert_eq!(first, second);
}
