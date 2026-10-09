//! The sandbox module's tests (a `tests.rs` file: the platform-seam
//! guard exempts this filename — the symlink test is unix-conditional
//! by nature, like every platform-boundary test).

use super::*;

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zamin-sandbox-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_file(path: &Path, len: usize) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, vec![0u8; len]).unwrap();
}

#[test]
fn classify_maps_the_three_verdicts_and_the_unbudgeted_case() {
    assert_eq!(classify_usage(10, Some(100)), StorageVerdict::Ok);
    assert_eq!(classify_usage(95, Some(100)), StorageVerdict::Warning);
    assert_eq!(classify_usage(100, Some(100)), StorageVerdict::Exceeded);
    assert_eq!(classify_usage(500, Some(100)), StorageVerdict::Exceeded);
    assert_eq!(classify_usage(500, None), StorageVerdict::Unbounded);
    // A zero budget is a wall, not a divide-by-zero.
    assert_eq!(classify_usage(0, Some(0)), StorageVerdict::Exceeded);
}

#[test]
fn measure_walks_the_tree_and_stops_at_the_budget() {
    let root = temp_root("walk");
    write_file(&root.join("a/logs/latest.log"), 300);
    write_file(&root.join("world/region/r.1.mca"), 700);
    let cancel = AtomicBool::new(false);

    let walk = measure_dir(&root, 10_000, &cancel).unwrap();
    assert_eq!(walk.bytes, 1000);
    assert_eq!(walk.files, 2);
    assert!(!walk.stopped_early);

    // A 600-byte budget stops the walk early — the bytes are a
    // lower bound by design, and the verdict that follows is
    // Exceeded.
    let walk = measure_dir(&root, 600, &cancel).unwrap();
    assert!(walk.stopped_early);
    assert!(walk.bytes >= 600);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn measure_never_follows_symlinks() {
    let root = temp_root("symlink");
    write_file(&root.join("inside/file.bin"), 500);
    let outside = temp_root("symlink-outside");
    write_file(&outside.join("huge.bin"), 100_000);
    std::os::unix::fs::symlink(outside.join("huge.bin"), root.join("inside/link.bin")).unwrap();
    let cancel = AtomicBool::new(false);
    let walk = measure_dir(&root, u64::MAX, &cancel).unwrap();
    assert_eq!(
        walk.bytes, 500,
        "a link out of the jail cannot smuggle its target's bytes in"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn journal_records_and_reads_back_oldest_first() {
    let root = temp_root("journal");
    let journal = SecurityJournal::new(&root);
    journal
        .record(Some("survival"), "storage_limit_reached", "past 100 GiB")
        .unwrap();
    journal
        .record(None, "suspicious_resource_usage", "cpu saturated")
        .unwrap();
    let entries = read_journal(&root.join("security.log"), 100);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["kind"], "storage_limit_reached");
    assert_eq!(entries[1]["serverId"], serde_json::Value::Null);
    // The cap keeps the read bounded; the newest survive.
    let capped = read_journal(&root.join("security.log"), 1);
    assert_eq!(capped.len(), 1);
    assert_eq!(capped[0]["kind"], "suspicious_resource_usage");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn journal_read_of_a_missing_file_is_empty_not_an_error() {
    let root = temp_root("journal-missing");
    let entries = read_journal(&root.join("security.log"), 100);
    assert!(entries.is_empty());
    let _ = std::fs::remove_dir_all(root);
}
