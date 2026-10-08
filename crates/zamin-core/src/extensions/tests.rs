//! The extension inventory's rules (ADR-0031): deny-by-default
//! permissions, slug ids, bounded claims, symlinks never followed, and
//! a listing that answers for everything it saw.

use std::fs;

use crate::error::CoreError;
use crate::extensions::{list_extensions, load_manifest, ExtensionListing, Permission};
use crate::server::registry::tempdir;

fn write_manifest(dir: &std::path::Path, body: &str) {
    fs::create_dir_all(dir).expect("creates the extension dir");
    fs::write(dir.join(super::MANIFEST_FILE), body).expect("writes the manifest");
}

const VALID: &str = r#"
id = "export-config"
name = "Export configuration"
version = "0.3.0"
description = "Adds an export action to the server context menu."
permissions = ["contribution:context-menu", "data:files.read"]
"#;

#[test]
fn a_valid_manifest_parses_with_its_permissions_declared() {
    let temp = tempdir::scoped("ext-valid");
    let dir = temp.path.join("ext");
    write_manifest(&dir, VALID);
    let manifest = load_manifest(&dir).expect("valid manifest parses");
    assert_eq!(manifest.id, "export-config");
    assert_eq!(manifest.name, "Export configuration");
    assert_eq!(manifest.version, "0.3.0");
    assert_eq!(
        manifest.permissions,
        vec![Permission::ContextMenu, Permission::FilesRead]
    );
}

#[test]
fn an_unknown_permission_is_rejected_wherever_it_appears() {
    // The typed parser (for programmatic users):
    let err = Permission::parse("data:*").expect_err("unknown permission rejected");
    assert!(
        matches!(err, CoreError::InvalidExtensionPermission { ref permission } if permission == "data:*"),
        "{err}"
    );
    // And the manifest path: an unrecognized permission is a rejection,
    // naming what was refused — never a shrug.
    let temp = tempdir::scoped("ext-unknown-perm");
    let dir = temp.path.join("ext");
    write_manifest(
        &dir,
        r#"
id = "greedy"
name = "Greedy"
version = "1.0.0"
permissions = ["data:*", "contribution:context-menu"]
"#,
    );
    let err = load_manifest(&dir).expect_err("unknown permission rejected");
    assert!(err.to_string().contains("data:*"), "{err}");
}

#[test]
fn ids_follow_the_slug_rules() {
    let temp = tempdir::scoped("ext-ids");
    let write_and_load = |body: &str| {
        let dir = temp.path.join("case");
        let _ = fs::remove_dir_all(&dir);
        write_manifest(&dir, body);
        load_manifest(&dir)
    };
    for bad in ["", "Has Spaces", "-leads", "über", "Upper"] {
        let body = format!(
            r#"
id = "{bad}"
name = "Whatever"
version = "1.0.0"
"#
        );
        assert!(
            write_and_load(&body).is_err(),
            "id {bad:?} must be rejected"
        );
    }
    let long_id = "a".repeat(65);
    let body = format!(
        r#"
id = "{long_id}"
name = "Whatever"
version = "1.0.0"
"#
    );
    assert!(write_and_load(&body).is_err(), "65-char id rejected");

    let dir = temp.path.join("good-id");
    write_manifest(
        &dir,
        r#"
id = "a1-_x"
name = "Dashes and underscores are fine"
version = "1.0.0"
"#,
    );
    assert!(load_manifest(&dir).is_ok(), "slug ids pass");
}

#[test]
fn duplicate_permissions_fold_and_bounds_hold() {
    let temp = tempdir::scoped("ext-bounds");
    let dir = temp.path.join("dup");
    write_manifest(
        &dir,
        r#"
id = "dup"
name = "  Padded name  "
version = " 2.0.0 "
permissions = ["data:files.read", "data:files.read"]
description = "  padded description  "
"#,
    );
    let manifest = load_manifest(&dir).expect("parses");
    assert_eq!(manifest.permissions, vec![Permission::FilesRead]);
    assert_eq!(manifest.name, "Padded name");
    assert_eq!(manifest.version, "2.0.0");
    assert_eq!(manifest.description.as_deref(), Some("padded description"));

    let long_name = "n".repeat(65);
    let dir = temp.path.join("longname");
    write_manifest(
        &dir,
        &format!(
            r#"
id = "longname"
name = "{long_name}"
version = "1.0.0"
"#
        ),
    );
    let err = load_manifest(&dir).expect_err("65-char name rejected");
    assert!(
        matches!(err, CoreError::InvalidExtensionManifest { .. }),
        "{err}"
    );
}

#[test]
fn the_listing_answers_for_every_folder_it_saw() {
    let temp = tempdir::scoped("ext-listing");
    let root = temp.path.join("extensions");
    write_manifest(&root.join("good"), VALID);
    write_manifest(&root.join("broken"), "id = 7\n");
    // A folder with no manifest at all — evidence, not a silent skip.
    fs::create_dir_all(root.join("nomanifest")).expect("dir");
    fs::write(root.join("nomanifest/notes.txt"), "placeholder").expect("notes");
    fs::write(root.join("loose.txt"), "not a folder").expect("loose file");
    // A symlinked folder is never followed (ADR-0009's rule, kept here).
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("good"), root.join("linked"))
        .expect("symlink for the refusal test");

    let listings = list_extensions(&root);

    // Deterministic order by directory name; the symlink and the loose
    // file are simply absent — never followed, never listed.
    let names: Vec<String> = listings
        .iter()
        .map(|l| match l {
            ExtensionListing::Valid { manifest, .. } => format!("ok:{}", manifest.id),
            ExtensionListing::Invalid { directory, .. } => format!("bad:{directory}"),
        })
        .collect();
    assert_eq!(
        names,
        vec![
            "bad:broken".to_string(),
            "ok:export-config".to_string(),
            "bad:nomanifest".to_string(),
        ],
        "sorted, answered for everything seen"
    );

    let problems: Vec<(String, String)> = listings
        .into_iter()
        .filter_map(|l| match l {
            ExtensionListing::Invalid { directory, reason } => Some((directory, reason)),
            _ => None,
        })
        .collect();
    assert!(
        problems
            .iter()
            .any(|(d, r)| d == "broken" && r.contains("parse")),
        "{problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|(d, r)| d == "nomanifest" && r.contains("unreadable")),
        "{problems:?}"
    );
}

#[test]
fn a_missing_extensions_dir_is_an_empty_inventory() {
    let temp = tempdir::scoped("ext-absent");
    assert!(list_extensions(&temp.path.join("absent")).is_empty());
}
