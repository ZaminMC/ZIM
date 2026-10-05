//! Filesystem safety tests (ADR-0009). These are the enforcement of the
//! security model — skipping one is a security regression, not a coverage
//! gap.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use crate::error::CoreError;
use crate::fsops::{EntryKind, RootedFs};
use crate::server::registry::tempdir;

fn fixture(tag: &str) -> (RootedFs, tempdir::TempDirGuard) {
    let dir = tempdir::scoped(tag);
    let fs = RootedFs::open(&dir.path).expect("root opens");
    (fs, dir)
}

#[test]
fn read_write_round_trip() {
    let (fs, _guard) = fixture("rw");
    fs.write("server.properties", b"online-mode=true\n")
        .unwrap();
    assert_eq!(
        fs.read("server.properties", 1024).unwrap(),
        b"online-mode=true\n"
    );
}

#[test]
fn atomic_write_replaces_existing_file() {
    let (fs, _guard) = fixture("atomic");
    fs.write("config.toml", b"one").unwrap();
    fs.write("config.toml", b"two").unwrap();
    assert_eq!(fs.read("config.toml", 16).unwrap(), b"two");
    // No temp leftovers.
    assert!(fs
        .list(".")
        .unwrap()
        .iter()
        .all(|e| !e.name.contains("zamin-tmp")));
}

#[test]
fn traversal_is_rejected_textually() {
    let (fs, _guard) = fixture("traversal");
    for rel in [
        "../escape",
        "a/../../b",
        "..",
        "/absolute",
        "C:/win",
        "back\\slash",
        "nul\0byte",
    ] {
        let err = fs.resolve(rel).unwrap_err();
        assert!(
            matches!(err, CoreError::PathEscapesRoot { .. }),
            "{rel:?} must be rejected"
        );
    }
}

#[test]
fn dot_segments_and_double_slashes_normalize() {
    let (fs, _guard) = fixture("dots");
    fs.write("plugins/thing.jar", b"x").unwrap();
    assert!(fs.resolve("./plugins//thing.jar").is_ok());
    assert!(fs.resolve("./plugins/./thing.jar").is_ok());
}

#[test]
#[cfg(unix)]
fn symlink_escape_is_denied() {
    let (fs, guard) = fixture("symlink");
    std::fs::write(guard.path.join("secret.txt"), b"outside").unwrap();
    let outside = std::env::temp_dir().join("zamin-escape-target");
    let _ = std::fs::remove_file(&outside);
    std::fs::write(&outside, b"outside").unwrap();

    std::os::unix::fs::symlink(&outside, guard.path.join("link")).unwrap();
    match fs.resolve("link") {
        Err(CoreError::PathEscapesRoot { .. }) => {}
        other => panic!("symlink escape must be denied, got {other:?}"),
    }
    // Reading through the link is denied by the same check.
    assert!(fs.read("link", 16).is_err());
    let _ = std::fs::remove_file(&outside);
}

#[test]
#[cfg(unix)]
fn symlink_inside_root_is_allowed_and_listed() {
    let (fs, guard) = fixture("symlink-inside");
    std::fs::write(guard.path.join("real.txt"), b"hi").unwrap();
    std::os::unix::fs::symlink("real.txt", guard.path.join("alias.txt")).unwrap();
    let entries = fs.list(".").unwrap();
    let alias = entries
        .iter()
        .find(|e| e.name == "alias.txt")
        .expect("listed");
    assert!(!alias.symlink_outside);
    assert_eq!(fs.read("alias.txt", 16).unwrap(), b"hi");
}

#[test]
fn listing_is_depth_one_and_sorted_dirs_first() {
    let (fs, _guard) = fixture("list");
    fs.write("b.txt", b"").unwrap();
    fs.write("a.txt", b"12345").unwrap();
    fs.mkdir("world").unwrap();

    let entries = fs.list(".").unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, "world");
    assert_eq!(entries[0].kind, EntryKind::Dir);
    assert_eq!(entries[1].name, "a.txt");
    assert_eq!(entries[1].size, Some(5));
}

#[test]
fn read_size_limit_is_enforced() {
    let (fs, _guard) = fixture("sizelimit");
    fs.write("big.bin", vec![0u8; 128].as_slice()).unwrap();
    match fs.read("big.bin", 16) {
        Err(CoreError::ReadTooLarge { .. }) => {}
        other => panic!("expected ReadTooLarge, got {other:?}"),
    }
}

#[test]
fn delete_file_and_empty_dir_only() {
    let (fs, _guard) = fixture("delete");
    fs.write("a.txt", b"").unwrap();
    fs.delete("a.txt").unwrap();
    assert!(!fs.resolve("a.txt").unwrap().exists(), "file must be gone");

    fs.mkdir("full").unwrap();
    fs.write("full/inner.txt", b"").unwrap();
    assert!(fs.delete("full").is_err(), "non-empty dir must not delete");
}

#[test]
fn rename_stays_contained() {
    let (fs, _guard) = fixture("rename");
    fs.write("old.txt", b"data").unwrap();
    fs.mkdir("sub").unwrap();
    fs.rename("old.txt", "sub/new.txt").unwrap();
    assert!(fs.read("sub/new.txt", 16).is_ok());
    assert!(fs.rename("sub/new.txt", "../../outside").is_err());
}

#[test]
fn write_into_new_subdirectory_creates_parents() {
    let (fs, _guard) = fixture("mkdirs");
    fs.write("deep/nested/file.txt", b"x").unwrap();
    assert_eq!(fs.read("deep/nested/file.txt", 8).unwrap(), b"x");
    // PathBuf sanity for the tempdir guard drop.
    assert!(PathBuf::from(&_guard.path).exists());
}
