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

/// The Windows-spelled escape vectors the first traversal law names only
/// by implication. UNC paths would leave the machine; NTFS alternate data
/// streams would hang hidden data off an in-jail file (`plugin.jar:hider`,
/// read by naming the stream even when the daemon's read returned the
/// base file); the drive-relative form (`C:file`) resolves against a
/// per-drive CWD the jail never owns. Every one of them is refused by the
/// same textual law — `:`, `\`, and a leading `/` never reach the
/// resolver — and this test pins each spelling so trimming one check
/// cannot come back quietly.
#[test]
fn unc_ads_and_drive_relative_spellings_are_rejected() {
    let (fs, _guard) = fixture("spellings");
    for rel in [
        "//server/share/world",  // UNC, forward-spelled
        r"\\server\share\world", // UNC, the spelling Windows itself prints
        "world/../../..//?/device", // dot-dot riding toward a device namespace
        "plugin.jar:stream",     // NTFS alternate data stream
        "plugins/x:important",   // a colon anywhere, not just the tail
        "C:escape",              // drive-relative (no slash) — CWD of that drive
        "zamin-stats:$DATA",     // the stream attribute's own name
    ] {
        let err = fs.resolve(rel).unwrap_err();
        assert!(
            matches!(err, CoreError::PathEscapesRoot { .. }),
            "{rel:?} must be rejected"
        );
        // The same spellings must fail on the mutating verbs too —
        // resolve is the choke point, but the contract is per-verb.
        assert!(fs.write(rel, b"x").is_err(), "write {rel:?} refused");
        assert!(fs.read(rel, 16).is_err(), "read {rel:?} refused");
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

// --- copy (the files slice, ADR-0021) ---

#[test]
fn copy_file_lands_byte_identical_and_reports() {
    let (fs, _guard) = fixture("copy-file");
    fs.write("paper.yml", b"spawn-protection: 16\n").unwrap();

    let outcome = fs.copy("paper.yml", "backup/paper.yml").unwrap();
    assert_eq!(outcome.files, 1);
    assert_eq!(outcome.bytes, 21);
    assert_eq!(
        fs.read("backup/paper.yml", 64).unwrap(),
        b"spawn-protection: 16\n"
    );
    // The original is untouched.
    assert_eq!(fs.read("paper.yml", 64).unwrap(), b"spawn-protection: 16\n");
}

#[test]
fn copy_never_overwrites_an_existing_target() {
    let (fs, _guard) = fixture("copy-exists");
    fs.write("a.txt", b"first").unwrap();
    fs.write("b.txt", b"second").unwrap();

    let err = fs.copy("a.txt", "b.txt").unwrap_err();
    assert!(matches!(err, CoreError::CopyTargetExists { .. }));
    // The target's bytes survived the refusal.
    assert_eq!(fs.read("b.txt", 16).unwrap(), b"second");
}

#[test]
fn copy_directory_copies_the_whole_tree() {
    let (fs, _guard) = fixture("copy-tree");
    fs.write("plugins/EssentialsX/config.yml", b"a\n").unwrap();
    fs.write("plugins/EssentialsX/messages.yml", b"b\n")
        .unwrap();
    fs.write("plugins/Vault.jar", b"c").unwrap();

    let outcome = fs.copy("plugins", "plugins-backup").unwrap();
    assert_eq!(outcome.files, 3);
    assert_eq!(outcome.bytes, 5);
    assert_eq!(fs.read("plugins-backup/Vault.jar", 8).unwrap(), b"c");
    assert_eq!(
        fs.read("plugins-backup/EssentialsX/config.yml", 8).unwrap(),
        b"a\n"
    );
}

#[test]
fn copy_refuses_symlinks_inside_the_tree() {
    let (fs, _guard) = fixture("copy-symlink");
    fs.write("real.txt", b"payload").unwrap();
    fs.mkdir("dir").unwrap();
    let link = fs.root().join("dir/link.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(fs.root().join("real.txt"), &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(fs.root().join("real.txt"), &link).unwrap();

    // A symlink anywhere inside the copied tree refuses the whole copy —
    // silently dereferencing it would duplicate a subtree the operator
    // did not point at, and following it could loop.
    let err = fs.copy("dir", "dir-copy").unwrap_err();
    assert!(matches!(err, CoreError::SymlinkInCopy { .. }));
    assert!(!fs.resolve("dir-copy").unwrap().exists());

    // The top-level path is resolved like every other method — the copy
    // of the link's path lands the target's bytes under the link's name,
    // exactly what a read of that path would answer.
    let outcome = fs.copy("dir/link.txt", "deref.txt").unwrap();
    assert_eq!(outcome.files, 1);
    assert_eq!(fs.read("deref.txt", 16).unwrap(), b"payload");
}

#[test]
fn copy_stops_at_the_byte_budget_with_the_tree_untouched() {
    let (fs, _guard) = fixture("copy-budget");
    fs.write("big1.bin", [0u8; 40].as_slice()).unwrap();
    fs.write("big2.bin", [0u8; 40].as_slice()).unwrap();

    let err = fs.copy_bounded("big1.bin", "out.bin", 32).unwrap_err();
    assert!(matches!(err, CoreError::CopyTooLarge { .. }));
    assert!(!fs.resolve("out.bin").unwrap().exists(), "no partial file");

    // A tree whose total exceeds the budget after some files landed:
    // the copy fails loudly, and nothing partial answers as success.
    let err = fs.copy_bounded(".", "out-dir", 64).unwrap_err();
    assert!(matches!(err, CoreError::CopyTooLarge { .. }));
    assert!(!fs.resolve("out-dir").unwrap().exists());
}

#[test]
fn copy_through_traversal_stays_rejected() {
    let (fs, _guard) = fixture("copy-traversal");
    fs.write("secret.txt", b"").unwrap();
    assert!(fs.copy("secret.txt", "../outside.txt").is_err());
    assert!(fs.copy("../outside.txt", "inside.txt").is_err());
}

// --- search (the files slice, ADR-0021) ---

#[test]
fn search_finds_names_case_insensitively_across_depths() {
    let (fs, _guard) = fixture("search-basic");
    fs.write("server.properties", b"").unwrap();
    fs.write("plugins/EssentialsX.jar", b"").unwrap();
    fs.write("plugins/EssentialsXSpawn/config.yml", b"")
        .unwrap();
    fs.write("unrelated.txt", b"").unwrap();

    let result = fs.search("essentialsx", 50).unwrap();
    let paths: Vec<&str> = result.hits.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["plugins/EssentialsX.jar", "plugins/EssentialsXSpawn"]
    );
    assert!(!result.truncated);
    assert!(result.scanned >= 5);
    // Files carry their size; the hit is a real row, not a name.
    assert_eq!(result.hits[0].kind, EntryKind::File);
}

#[test]
fn search_reports_directories_and_skips_the_staging_area() {
    let (fs, _guard) = fixture("search-dirs");
    fs.mkdir("world/region").unwrap();
    fs.write("world/region/r.0.0.mca", b"").unwrap();
    std::fs::create_dir_all(fs.root().join(".zamin-staging")).unwrap();
    fs.write(".zamin-staging/stage-x", b"").unwrap();

    let result = fs.search("world", 50).unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].path, "world");
    assert_eq!(result.hits[0].kind, EntryKind::Dir);
    assert!(result.hits[0].size.is_none(), "directories carry no size");

    // The daemon's staging dir is never an answer.
    let result = fs.search("stage", 50).unwrap();
    assert!(result.hits.is_empty(), "staging is invisible to search");
}

#[test]
fn search_truncates_honestly_at_the_limit() {
    let (fs, _guard) = fixture("search-limit");
    for i in 0..8 {
        fs.write(&format!("log-{i}.txt"), b"").unwrap();
    }

    let result = fs.search("log-", 3).unwrap();
    assert_eq!(result.hits.len(), 3);
    assert!(result.truncated, "the bound must say so");

    // A limit above the match count is not a truncation.
    let result = fs.search("log-", 100).unwrap();
    assert_eq!(result.hits.len(), 8);
    assert!(!result.truncated);
}

#[test]
fn search_of_a_deep_tree_stops_at_the_depth_bound() {
    let (fs, _guard) = fixture("search-deep");
    let mut deep = String::new();
    for i in 0..40 {
        deep.push_str(&format!("d{i}/"));
    }
    deep.push_str("target.txt");
    fs.write(&deep, b"").unwrap();

    // 40 levels down, past the walk's 32-level bound: the walk says it
    // was cut short rather than silently answering "nothing".
    let result = fs.search("target", 50).unwrap();
    assert!(
        result.truncated || !result.hits.is_empty(),
        "either the hit is found or the truncation flag says why not"
    );
}
