//! Tests for the backup/restore core: the roundtrip, every ADR-0009
//! restore trap as a typed rejection, cancel, disk-full classification,
//! retention, and the commit rollback.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::restore::{commit, CommitError};
use super::*;
use crate::server::registry::tempdir;

fn quiet_progress() -> Arc<dyn Fn(CreateProgress) + Send + Sync> {
    Arc::new(|_| {})
}

fn quiet_restore_progress() -> Arc<dyn Fn(RestoreProgress) + Send + Sync> {
    Arc::new(|_| {})
}

fn opts(
    server_id: &str,
    cancel: &Arc<AtomicBool>,
    progress: Arc<dyn Fn(CreateProgress) + Send + Sync>,
) -> BackupCreateOptions {
    BackupCreateOptions {
        server_id: server_id.to_owned(),
        label: None,
        taken: BackupTaken::Cold,
        cancel: Arc::clone(cancel),
        progress,
    }
}

fn restore_opts(
    cancel: &Arc<AtomicBool>,
    progress: Arc<dyn Fn(RestoreProgress) + Send + Sync>,
) -> RestoreOptions {
    RestoreOptions {
        cancel: Arc::clone(cancel),
        progress,
        max_entries: 0,
        max_total_bytes: 0,
    }
}

/// A believable server root: configs, a world tree, a marker, logs.
fn seed_server(root: &Path) {
    fs::create_dir_all(root.join("world/region")).unwrap();
    fs::create_dir_all(root.join("plugins")).unwrap();
    fs::create_dir_all(root.join(".zamin")).unwrap();
    fs::create_dir_all(root.join("logs")).unwrap();
    fs::write(root.join("eula.txt"), "eula=true\n").unwrap();
    fs::write(root.join("server.properties"), "motd=Hello\n").unwrap();
    fs::write(root.join("world/level.dat"), b"\x0a\x00binary").unwrap();
    fs::write(root.join("world/region/r.0.0.mca"), vec![7u8; 4096]).unwrap();
    fs::write(root.join("plugins/EssentialsX.jar"), vec![1u8; 2048]).unwrap();
    fs::write(root.join(".zamin/server.json"), b"{\"schemaVersion\":1}").unwrap();
    fs::write(root.join("logs/latest.log"), "Done (3.2s)!\n").unwrap();
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn assert_same_tree(a: &Path, b: &Path) {
    assert_eq!(snapshot(a), snapshot(b), "trees must be identical");
}

#[test]
fn create_then_restore_roundtrip_is_lossless() {
    let dir = tempdir::scoped("backup-roundtrip");
    let root = dir.path.join("server");
    let fresh = dir.path.join("fresh");
    fs::create_dir_all(&root).unwrap();
    seed_server(&root);

    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = create_archive(
        &root,
        &dir.path.join("backups"),
        opts("demo", &cancel, quiet_progress()),
    )
    .unwrap();

    let m = &outcome.manifest;
    assert_eq!(m.format_version, ARCHIVE_FORMAT_VERSION);
    assert_eq!(m.server_id, "demo");
    // eula.txt, server.properties, world/level.dat, world/region/r.0.0.mca,
    // plugins/EssentialsX.jar, .zamin/server.json, logs/latest.log.
    assert_eq!(m.file_count, 7);
    assert!(m.total_bytes > 0);
    assert!(m.size_bytes > 0);
    assert!(outcome.archive.is_file());
    assert!(manifest_path(&dir.path.join("backups"), m.backup_id).is_file());
    // The manifest on disk is readable and equals the returned one.
    let on_disk = list_backups(&dir.path.join("backups"));
    assert_eq!(on_disk.len(), 1);
    assert_eq!(on_disk[0], *m);

    // Restore into an empty root and compare.
    fs::create_dir_all(&fresh).unwrap();
    let progress = quiet_restore_progress();
    let ropts = restore_opts(&cancel, Arc::clone(&progress));
    let restored = restore_archive(&fresh, &outcome.archive, &ropts).unwrap();
    assert_eq!(restored.restored_files, m.file_count);
    assert_eq!(restored.restored_bytes, m.total_bytes);
    assert_same_tree(&root, &fresh);
    // No restore workspace leftovers in the fresh root.
    assert!(!fs::read_dir(&fresh).unwrap().flatten().any(|e| e
        .file_name()
        .to_string_lossy()
        .starts_with(".zamin-restore-")));
}

#[test]
fn create_counts_files_exactly() {
    let dir = tempdir::scoped("backup-count");
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.txt"), "hello").unwrap();
    fs::write(root.join("b.txt"), "world!").unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = create_archive(
        &root,
        &dir.path.join("backups"),
        opts("c", &cancel, quiet_progress()),
    )
    .unwrap();
    assert_eq!(outcome.manifest.file_count, 2);
    assert_eq!(outcome.manifest.total_bytes, 11);
}

#[test]
fn restore_replaces_a_modified_root() {
    let dir = tempdir::scoped("backup-replace");
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    seed_server(&root);

    let cancel = Arc::new(AtomicBool::new(false));
    let archive = create_archive(
        &root,
        &dir.path.join("backups"),
        opts("demo", &cancel, quiet_progress()),
    )
    .unwrap()
    .archive;

    // Mutate: edit, add, delete.
    fs::write(root.join("server.properties"), "motd=TAMPERED\n").unwrap();
    fs::write(root.join("rogue.txt"), "extra").unwrap();
    fs::remove_file(root.join("plugins/EssentialsX.jar")).unwrap();

    let progress = quiet_restore_progress();
    let ropts = restore_opts(&cancel, Arc::clone(&progress));
    restore_archive(&root, &archive, &ropts).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("server.properties")).unwrap(),
        "motd=Hello\n"
    );
    assert!(
        !root.join("rogue.txt").exists(),
        "files not in the backup are gone"
    );
    assert!(root.join("plugins/EssentialsX.jar").exists());
}

#[test]
fn staging_dirs_and_marker_rules_hold() {
    let dir = tempdir::scoped("backup-exclude");
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("keep.txt"), "x").unwrap();
    fs::create_dir_all(root.join(".zamin-staging")).unwrap();
    fs::write(root.join(".zamin-staging/partial"), "junk").unwrap();
    fs::create_dir_all(root.join(".zamin-restore-staging-abc")).unwrap();
    fs::create_dir_all(root.join(".zamin-backup-staging-abc.tar.gz.d")).unwrap();
    fs::create_dir_all(root.join(".zamin")).unwrap();
    fs::write(root.join(".zamin/server.json"), b"{}").unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = create_archive(
        &root,
        &dir.path.join("backups"),
        opts("e", &cancel, quiet_progress()),
    )
    .unwrap();
    // keep.txt + the marker file; staging excluded, marker INCLUDED.
    assert_eq!(outcome.manifest.file_count, 2);
}

#[cfg(unix)]
#[test]
fn symlinks_are_never_archived() {
    let dir = tempdir::scoped("backup-symlink");
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("real.txt"), "data").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", root.join("outside-link")).unwrap();
    std::os::unix::fs::symlink("real.txt", root.join("inside-link")).unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = create_archive(
        &root,
        &dir.path.join("backups"),
        opts("s", &cancel, quiet_progress()),
    )
    .unwrap();
    assert_eq!(
        outcome.manifest.file_count, 1,
        "links are skipped, never followed"
    );
}

#[test]
fn empty_root_backs_up_and_restores_clean() {
    let dir = tempdir::scoped("backup-empty");
    let empty = dir.path.join("empty");
    fs::create_dir_all(&empty).unwrap();
    // The restore target holds junk; an empty backup must clear it.
    let target = dir.path.join("server");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("doomed.txt"), "gone after restore").unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let archive = create_archive(
        &empty,
        &dir.path.join("backups"),
        opts("n", &cancel, quiet_progress()),
    )
    .unwrap()
    .archive;
    assert_eq!(
        list_backups(&dir.path.join("backups"))[0].file_count,
        0,
        "an empty root archives zero files"
    );

    let progress = quiet_restore_progress();
    restore_archive(
        &target,
        &archive,
        &restore_opts(&cancel, Arc::clone(&progress)),
    )
    .unwrap();
    assert!(
        !target.join("doomed.txt").exists(),
        "restore replaces the whole tree"
    );
}

#[test]
fn deep_paths_roundtrip_through_gnu_longnames() {
    let dir = tempdir::scoped("backup-deep");
    let root = dir.path.join("server");
    let deep_rel = "world/region/deeply/nested/branch/of/a/very/long/world/directory/tree/that/blows/past/the/hundred/byte/limit/for/sure/r.0.0.mca";
    fs::create_dir_all(root.join(deep_rel).parent().unwrap()).unwrap();
    fs::write(root.join(deep_rel), vec![9u8; 512]).unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let archive = create_archive(
        &root,
        &dir.path.join("backups"),
        opts("d", &cancel, quiet_progress()),
    )
    .unwrap()
    .archive;
    let fresh = dir.path.join("fresh");
    fs::create_dir_all(&fresh).unwrap();
    restore_archive(
        &fresh,
        &archive,
        &restore_opts(&cancel, quiet_restore_progress()),
    )
    .unwrap();
    assert_same_tree(&root, &fresh);
}

#[test]
fn progress_is_reported_for_files() {
    let dir = tempdir::scoped("backup-progress");
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a"), vec![0; 100]).unwrap();
    fs::write(root.join("b"), vec![0; 200]).unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let progress: Arc<dyn Fn(CreateProgress) + Send + Sync> = Arc::new(move |p| {
        sink.lock().unwrap().push(p);
    });
    let outcome = create_archive(
        &root,
        &dir.path.join("backups"),
        opts("p", &cancel, progress),
    )
    .unwrap();
    let seen = seen.lock().unwrap();
    let last = seen.last().unwrap();
    assert_eq!(last.files_done, outcome.manifest.file_count);
    assert_eq!(last.bytes_done, outcome.manifest.total_bytes);
    assert!(seen.windows(2).all(|w| w[0].files_done <= w[1].files_done));
}

// --- cancellation ---------------------------------------------------------

#[test]
fn cancel_mid_create_is_typed_and_cleans_up() {
    let dir = tempdir::scoped("backup-cancel-create");
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    seed_server(&root);
    let backups = dir.path.join("backups");

    let cancel = Arc::new(AtomicBool::new(true)); // cancelled before it starts
    let err = create_archive(&root, &backups, opts("x", &cancel, quiet_progress())).unwrap_err();
    assert!(matches!(err, crate::error::CoreError::Cancelled));
    // Nothing leaked into the listing and no staging file survived.
    assert!(list_backups(&backups).is_empty());
    let leftovers: Vec<_> = fs::read_dir(&backups)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(leftovers.is_empty(), "no staging leftovers: {leftovers:?}");
}

#[test]
fn cancel_mid_restore_is_typed_and_leaves_root_alone() {
    let dir = tempdir::scoped("backup-cancel-restore");
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    seed_server(&root);
    let cancel = Arc::new(AtomicBool::new(false));
    let archive = create_archive(
        &root,
        &dir.path.join("backups"),
        opts("x", &cancel, quiet_progress()),
    )
    .unwrap()
    .archive;

    let stop = Arc::new(AtomicBool::new(true));
    let err = restore_archive(
        &root,
        &archive,
        &restore_opts(&stop, quiet_restore_progress()),
    )
    .unwrap_err();
    assert!(matches!(err, crate::error::CoreError::Cancelled));
    assert!(root.join("eula.txt").exists(), "the live root is untouched");
    assert!(!fs::read_dir(&root).unwrap().flatten().any(|e| e
        .file_name()
        .to_string_lossy()
        .starts_with(".zamin-restore-")));
}

// --- restore traps (crafted archives) --------------------------------------

/// Craft an arbitrary tar.gz with the given (name, content) file entries.
/// The tar crate refuses to WRITE dangerous names, so the raw header bytes
/// are filled by hand — exactly what a malicious producer would do.
fn craft_archive(path: &Path, entries: &[(&str, &[u8])]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let file = fs::File::create(path).unwrap();
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    let mut builder = tar::Builder::new(gz);
    for (name, content) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        {
            // Write the name field directly; set_path would refuse.
            let gnu = header.as_gnu_mut().unwrap();
            gnu.name = [0u8; 100];
            gnu.name[..name.len()].copy_from_slice(name.as_bytes());
        }
        header.set_cksum();
        builder.append(&header, *content).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap();
}

fn restore_error(root: &Path, archive: &Path) -> crate::error::CoreError {
    let cancel = Arc::new(AtomicBool::new(false));
    restore_archive(
        root,
        archive,
        &restore_opts(&cancel, quiet_restore_progress()),
    )
    .unwrap_err()
}

fn assert_root_untouched(root: &Path) {
    assert!(root.join("sentinel.txt").exists(), "the live root survived");
    assert!(
        !fs::read_dir(root).unwrap().flatten().any(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with(".zamin-restore-")),
        "staging was cleaned"
    );
}

fn seeded_root_with_sentinel(tag: &str) -> (tempdir::TempDirGuard, PathBuf) {
    let dir = tempdir::scoped(tag);
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("sentinel.txt"), "precious").unwrap();
    (dir, root)
}

#[test]
fn zip_slip_entry_is_rejected_and_nothing_escapes() {
    let (dir, root) = seeded_root_with_sentinel("restore-zipslip");
    let archive = dir.path.join("evil.tar.gz");
    craft_archive(&archive, &[("../evil.txt", b"escaped")]);

    let err = restore_error(&root, &archive);
    assert!(
        matches!(err, crate::error::CoreError::ArchiveUnsafeEntry { .. }),
        "{err}"
    );
    assert_root_untouched(&root);
    assert!(
        !dir.path.join("evil.txt").exists(),
        "nothing escaped the root"
    );
    assert!(!root.join("evil.txt").exists());
}

#[test]
fn absolute_path_entry_is_rejected() {
    let (dir, root) = seeded_root_with_sentinel("restore-absolute");
    let archive = dir.path.join("evil.tar.gz");
    craft_archive(&archive, &[("/etc/evil", b"nope")]);
    let err = restore_error(&root, &archive);
    assert!(
        matches!(err, crate::error::CoreError::ArchiveUnsafeEntry { .. }),
        "{err}"
    );
    assert_root_untouched(&root);
}

#[cfg(unix)] // the crafted name uses backslashes as literal bytes
#[test]
fn backslash_entry_is_rejected() {
    let (dir, root) = seeded_root_with_sentinel("restore-backslash");
    let archive = dir.path.join("evil.tar.gz");
    craft_archive(&archive, &[(r"world\..\..\evil", b"nope")]);
    let err = restore_error(&root, &archive);
    assert!(
        matches!(err, crate::error::CoreError::ArchiveUnsafeEntry { .. }),
        "{err}"
    );
    assert_root_untouched(&root);
}

#[test]
fn dot_components_are_rejected() {
    let (dir, root) = seeded_root_with_sentinel("restore-dot");
    let archive = dir.path.join("evil.tar.gz");
    craft_archive(&archive, &[("./evil", b"nope")]);
    let err = restore_error(&root, &archive);
    assert!(
        matches!(err, crate::error::CoreError::ArchiveUnsafeEntry { .. }),
        "{err}"
    );
    assert_root_untouched(&root);
}

#[test]
fn windows_reserved_names_are_rejected_everywhere() {
    let (dir, root) = seeded_root_with_sentinel("restore-reserved");
    let archive = dir.path.join("evil.tar.gz");
    // All of these would fail (or write to devices) on Windows: the plain
    // name, an extension variant, a different case, a nested component.
    craft_archive(
        &archive,
        &[
            ("CON", b"a"),
            ("NUL.txt", b"b"),
            ("world/com1.ZIP", b"c"),
            ("deep/LPT9/x", b"d"),
        ],
    );
    let err = restore_error(&root, &archive);
    assert!(
        matches!(err, crate::error::CoreError::ArchiveUnsafeEntry { .. }),
        "{err}"
    );
    assert_root_untouched(&root);
}

#[test]
fn reserved_lookalikes_are_allowed() {
    let (dir, root) = seeded_root_with_sentinel("restore-reserved-ok");
    let archive = dir.path.join("ok.tar.gz");
    craft_archive(
        &archive,
        &[
            ("console.log", b"a"),
            ("auxiliary.txt", b"b"),
            ("world/nullop", b"c"),
        ],
    );
    let cancel = Arc::new(AtomicBool::new(false));
    restore_archive(
        &root,
        &archive,
        &restore_opts(&cancel, quiet_restore_progress()),
    )
    .unwrap();
    assert!(root.join("console.log").exists());
    assert!(root.join("world/nullop").exists());
}

#[test]
fn case_insensitive_collision_is_rejected() {
    let (dir, root) = seeded_root_with_sentinel("restore-case");
    let archive = dir.path.join("evil.tar.gz");
    craft_archive(
        &archive,
        &[("World/level.dat", b"a"), ("world/level.dat", b"b")],
    );
    let err = restore_error(&root, &archive);
    assert!(
        matches!(err, crate::error::CoreError::ArchiveUnsafeEntry { .. }),
        "{err}"
    );
    assert_root_untouched(&root);
}

#[test]
fn entry_and_size_limits_are_enforced() {
    let (dir, root) = seeded_root_with_sentinel("restore-limits");
    let archive = dir.path.join("big.tar.gz");
    craft_archive(
        &archive,
        &[("a", b"12345"), ("b", b"67890"), ("c", b"abcde")],
    );

    let stop = Arc::new(AtomicBool::new(false));
    let err = restore_archive(
        &root,
        &archive,
        &RestoreOptions {
            cancel: Arc::clone(&stop),
            progress: quiet_restore_progress(),
            max_entries: 2,
            max_total_bytes: 0,
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, crate::error::CoreError::ArchiveTooLarge { .. }),
        "{err}"
    );

    let err = restore_archive(
        &root,
        &archive,
        &RestoreOptions {
            cancel: Arc::clone(&stop),
            progress: quiet_restore_progress(),
            max_entries: 0,
            max_total_bytes: 10,
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, crate::error::CoreError::ArchiveTooLarge { .. }),
        "{err}"
    );
    assert_root_untouched(&root);
}

#[cfg(unix)]
#[test]
fn symlink_entries_are_rejected() {
    let (dir, root) = seeded_root_with_sentinel("restore-link");
    let archive = dir.path.join("evil.tar.gz");
    // Craft with a symlink entry directly.
    let file = fs::File::create(&archive).unwrap();
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    let mut builder = tar::Builder::new(gz);
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mtime(0);
    builder
        .append_data(&mut header, "link", std::io::empty())
        .unwrap();
    builder.into_inner().unwrap().finish().unwrap();

    let err = restore_error(&root, &archive);
    assert!(
        matches!(err, crate::error::CoreError::ArchiveUnsafeEntry { .. }),
        "{err}"
    );
    assert_root_untouched(&root);
}

// --- commit / rollback ------------------------------------------------------

#[test]
fn commit_failure_rolls_the_live_tree_back() {
    let dir = tempdir::scoped("restore-rollback");
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(root.join("world/region")).unwrap();
    fs::write(root.join("world/level.dat"), b"live").unwrap();
    fs::write(root.join("world/region/r.0.0.mca"), b"live-region").unwrap();
    fs::write(root.join("server.properties"), "live\n").unwrap();

    // A staged tree that would replace the world.
    let staging = dir.path.join("staging");
    fs::create_dir_all(staging.join("world")).unwrap();
    fs::write(staging.join("world/level.dat"), b"staged").unwrap();
    fs::write(staging.join("new.txt"), b"staged").unwrap();

    // Sabotage: the rollback dir already contains a non-empty `world`, so
    // moving the live `world` aside fails mid-commit.
    let rollback = dir.path.join("rollback");
    fs::create_dir_all(rollback.join("world")).unwrap();
    fs::write(rollback.join("world/blocker"), b"occupied").unwrap();

    let err = commit(&root, &staging, &rollback).unwrap_err();
    assert!(
        matches!(err, CommitError::RolledBack(_)),
        "a sane rollback is the expected failure: {err:?}"
    );

    // The live tree is exactly as it was.
    assert_eq!(fs::read(root.join("world/level.dat")).unwrap(), b"live");
    assert_eq!(
        fs::read(root.join("world/region/r.0.0.mca")).unwrap(),
        b"live-region"
    );
    assert_eq!(fs::read(root.join("server.properties")).unwrap(), b"live\n");
    assert!(!root.join("new.txt").exists(), "nothing staged leaked in");
    // Workspace dirs were cleaned.
    assert!(!staging.exists());
    assert!(!rollback.exists());
}

#[test]
fn commit_happy_path_swaps_the_trees() {
    let dir = tempdir::scoped("restore-commit-ok");
    let root = dir.path.join("server");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("old.txt"), b"old").unwrap();

    let staging = dir.path.join("staging");
    fs::create_dir_all(staging.join("world")).unwrap();
    fs::write(staging.join("new.txt"), b"new").unwrap();
    fs::write(staging.join("world/level.dat"), b"data").unwrap();

    let rollback = dir.path.join("rollback");
    commit(&root, &staging, &rollback).unwrap();

    assert!(!root.join("old.txt").exists());
    assert_eq!(fs::read(root.join("new.txt")).unwrap(), b"new");
    assert_eq!(fs::read(root.join("world/level.dat")).unwrap(), b"data");
    assert!(!staging.exists() && !rollback.exists(), "workspace cleaned");
}

// --- disk-full classification ------------------------------------------------

#[test]
fn disk_full_is_typed_not_generic_io() {
    let enospc = std::io::Error::from_raw_os_error(28);
    assert!(matches!(
        classify_io(PathBuf::from("/x"), enospc),
        crate::error::CoreError::DiskFull { .. }
    ));
    let windows_disk_full = std::io::Error::from_raw_os_error(112);
    assert!(matches!(
        classify_io(PathBuf::from("/x"), windows_disk_full),
        crate::error::CoreError::DiskFull { .. }
    ));
    let quota = std::io::Error::from_raw_os_error(122);
    assert!(matches!(
        classify_io(PathBuf::from("/x"), quota),
        crate::error::CoreError::DiskFull { .. }
    ));
    let other = std::io::Error::from_raw_os_error(13); // EACCES
    assert!(matches!(
        classify_io(PathBuf::from("/x"), other),
        crate::error::CoreError::Io { .. }
    ));
}

// --- listing & retention ------------------------------------------------------

fn write_fake_backup(backups: &Path, id: uuid::Uuid, created_at_ms: i64) {
    let manifest = BackupManifest {
        format_version: ARCHIVE_FORMAT_VERSION,
        backup_id: id,
        server_id: "demo".to_owned(),
        created_at_ms,
        size_bytes: 10,
        total_bytes: 10,
        file_count: 1,
        label: None,
        taken: BackupTaken::Cold,
    };
    fs::write(
        manifest_path(backups, id),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(archive_path(backups, id), b"fake archive").unwrap();
}

#[test]
fn listing_is_sorted_oldest_first_and_skips_orphans() {
    let dir = tempdir::scoped("backup-list");
    let backups = dir.path.join("backups");
    fs::create_dir_all(&backups).unwrap();
    let a = uuid::Uuid::now_v7();
    let b = uuid::Uuid::now_v7();
    let c = uuid::Uuid::now_v7();
    write_fake_backup(&backups, c, 3000);
    write_fake_backup(&backups, a, 1000);
    write_fake_backup(&backups, b, 2000);
    // Orphans: an unreadable manifest and an archive without one.
    fs::write(backups.join("broken.json"), b"not json").unwrap();
    fs::write(backups.join("lonely.tar.gz"), b"no manifest").unwrap();

    let listed = list_backups(&backups);
    let ids: Vec<_> = listed.iter().map(|m| m.backup_id).collect();
    assert_eq!(
        ids,
        vec![a, b, c],
        "sorted by created_at_ms, orphans skipped"
    );
}

#[test]
fn retention_keeps_the_newest_and_deletes_the_rest() {
    let dir = tempdir::scoped("backup-retention");
    let backups = dir.path.join("backups");
    fs::create_dir_all(&backups).unwrap();
    let ids: Vec<_> = (0..5)
        .map(|i| {
            let id = uuid::Uuid::now_v7();
            write_fake_backup(&backups, id, i * 1000);
            id
        })
        .collect();

    let pruned = prune_backups(&backups, 2).unwrap();
    assert_eq!(pruned, vec![ids[0], ids[1], ids[2]], "oldest first");
    let listed = list_backups(&backups);
    let kept: Vec<_> = listed.iter().map(|m| m.backup_id).collect();
    assert_eq!(kept, vec![ids[3], ids[4]]);
    for pruned_id in &pruned {
        assert!(!archive_path(&backups, *pruned_id).exists());
        assert!(!manifest_path(&backups, *pruned_id).exists());
    }
    for kept_id in &kept {
        assert!(archive_path(&backups, *kept_id).exists());
    }
}

#[test]
fn retention_under_the_limit_is_a_no_op() {
    let dir = tempdir::scoped("backup-retention-none");
    let backups = dir.path.join("backups");
    fs::create_dir_all(&backups).unwrap();
    write_fake_backup(&backups, uuid::Uuid::now_v7(), 1000);
    write_fake_backup(&backups, uuid::Uuid::now_v7(), 2000);
    assert!(prune_backups(&backups, 2).unwrap().is_empty());
    assert_eq!(list_backups(&backups).len(), 2);
}
