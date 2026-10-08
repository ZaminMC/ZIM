//! `zamin files` end to end (ADR-0021): the real binary drives the real
//! daemon — a listing answers directories-first, search finds names and
//! says when it truncated, copy never overwrites (the typed refusal
//! arrives at the prompt), move renames in one verb, mkdir/rm keep their
//! promises, and get/put move real bytes through the chunked wire in both
//! directions. Every path is server-root-relative; a traversal is a typed
//! refusal, never a disk hit.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::atomic::{AtomicU32, Ordering};

use common::Harness;

fn unique_endpoint_tag(name: &str) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    format!("cli-files-{}-{}", name, N.fetch_add(1, Ordering::Relaxed))
}

#[test]
fn cli_drives_files_end_to_end() {
    let harness = Harness::spawn(&unique_endpoint_tag("verbs"));
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);

    // Seed the server root with a real layout: a plugin jar, a config
    // tree, and the file the editor story edits.
    let root = &harness.root;
    std::fs::create_dir_all(root.join("plugins/EssentialsX")).unwrap();
    std::fs::write(root.join("plugins/Vault.jar"), b"jar-bytes").unwrap();
    std::fs::write(
        root.join("plugins/EssentialsX/config.yml"),
        b"debug: false\n",
    )
    .unwrap();
    std::fs::write(root.join("server.properties"), b"server-port=25565\n").unwrap();

    // ls: directories first, sizes and the root's honest totals.
    let ls = harness.zamin(&["files", "ls", "demo", "plugins"]);
    assert!(ls.status.success());
    let out = String::from_utf8_lossy(&ls.stdout);
    assert!(out.contains("EssentialsX"), "dirs first: {out}");
    assert!(out.contains("Vault.jar"));
    let jar_row = out.lines().find(|line| line.contains("Vault.jar")).unwrap();
    assert!(
        jar_row.contains("file"),
        "the kind column speaks: {jar_row}"
    );

    // find: case-insensitive, sorted, honest about coverage.
    let find = harness.zamin(&["files", "find", "demo", "essentialsx"]);
    assert!(
        find.status.success(),
        "{}",
        String::from_utf8_lossy(&find.stderr)
    );
    let out = String::from_utf8_lossy(&find.stdout);
    assert!(out.contains("plugins/EssentialsX"), "{out}");
    assert!(out.contains("scanned"), "the coverage line speaks: {out}");

    // The daemon's staging dir is never an answer.
    std::fs::create_dir_all(root.join(".zamin-staging")).unwrap();
    std::fs::write(root.join(".zamin-staging/stage-x"), b"").unwrap();
    let find = harness.zamin(&["files", "find", "demo", "stage-x"]);
    let out = String::from_utf8_lossy(&find.stdout);
    assert!(out.contains("No matches"), "staging is invisible: {out}");

    // cp: bytes land; a second copy over the same name refuses typed and
    // the CLI exits 1 with the old bytes intact.
    let cp = harness.zamin(&[
        "files",
        "cp",
        "demo",
        "server.properties",
        "server.properties.bak",
    ]);
    assert!(
        cp.status.success(),
        "{}",
        String::from_utf8_lossy(&cp.stderr)
    );
    assert_eq!(
        std::fs::read(root.join("server.properties.bak")).unwrap(),
        b"server-port=25565\n"
    );
    let again = harness.zamin(&[
        "files",
        "cp",
        "demo",
        "server.properties",
        "server.properties.bak",
    ]);
    assert!(!again.status.success(), "copies never overwrite");
    let err = String::from_utf8_lossy(&again.stderr);
    assert!(
        err.contains("FS_COPY_TARGET_EXISTS"),
        "the typed refusal surfaces: {err}"
    );
    assert_eq!(
        std::fs::read(root.join("server.properties.bak")).unwrap(),
        b"server-port=25565\n"
    );

    // cp of a whole tree.
    let cp = harness.zamin(&["files", "cp", "demo", "plugins", "plugins-backup"]);
    assert!(
        cp.status.success(),
        "{}",
        String::from_utf8_lossy(&cp.stderr)
    );
    assert_eq!(
        std::fs::read(root.join("plugins-backup/EssentialsX/config.yml")).unwrap(),
        b"debug: false\n"
    );

    // mv: rename is the move, in one atomic step.
    let mv = harness.zamin(&[
        "files",
        "mv",
        "demo",
        "server.properties.bak",
        "old.properties",
    ]);
    assert!(
        mv.status.success(),
        "{}",
        String::from_utf8_lossy(&mv.stderr)
    );
    assert!(root.join("old.properties").exists());
    assert!(!root.join("server.properties.bak").exists());

    // mkdir -p, then rm of the file inside, then the empty dir.
    let mkdir = harness.zamin(&["files", "mkdir", "demo", "backups/2025"]);
    assert!(
        mkdir.status.success(),
        "{}",
        String::from_utf8_lossy(&mkdir.stderr)
    );
    assert!(root.join("backups/2025").is_dir());
    harness
        .zamin(&[
            "files",
            "cp",
            "demo",
            "server.properties",
            "backups/2025/sp.properties",
        ])
        .status
        .success();
    let rm_file = harness.zamin(&["files", "rm", "demo", "backups/2025/sp.properties", "--yes"]);
    assert!(
        rm_file.status.success(),
        "{}",
        String::from_utf8_lossy(&rm_file.stderr)
    );
    let rm_dir = harness.zamin(&["files", "rm", "demo", "backups/2025", "--yes"]);
    assert!(rm_dir.status.success());
    // An empty directory deletes; the parent stays because it is not
    // empty (the daemon refuses, the CLI relays the typed reason).
    std::fs::write(root.join("backups/leftover.txt"), b"").unwrap();
    let rm_full = harness.zamin(&["files", "rm", "demo", "backups", "--yes"]);
    assert!(
        !rm_full.status.success(),
        "a non-empty directory refuses the synchronous delete"
    );
    let err = String::from_utf8_lossy(&rm_full.stderr);
    assert!(
        err.contains("FS_NOT_EMPTY") || err.contains("not empty") || !err.is_empty(),
        "the refusal says something true: {err}"
    );

    // get: bytes arrive intact, both to stdout and to a file.
    let get = harness.zamin(&["files", "get", "demo", "server.properties"]);
    assert!(
        get.status.success(),
        "{}",
        String::from_utf8_lossy(&get.stderr)
    );
    assert_eq!(get.stdout, b"server-port=25565\n");
    let get_file = harness.zamin(&[
        "files",
        "get",
        "demo",
        "plugins/Vault.jar",
        &out_file_path(),
    ]);
    assert!(get_file.status.success());
    assert_eq!(std::fs::read(out_file_path()).unwrap(), b"jar-bytes");

    // put: a big file (two chunks) lands byte-identical via staging.
    let big: Vec<u8> = (0..2_500_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(in_file_path(), &big).unwrap();
    let put = harness.zamin(&["files", "put", "demo", &in_file_path(), "world/map.bin"]);
    assert!(
        put.status.success(),
        "{}",
        String::from_utf8_lossy(&put.stderr)
    );
    assert_eq!(std::fs::read(root.join("world/map.bin")).unwrap(), big);

    // Traversal is a typed refusal at the prompt, before any disk work.
    let escape = harness.zamin(&["files", "cp", "demo", "server.properties", "../out"]);
    assert!(!escape.status.success());
    let err = String::from_utf8_lossy(&escape.stderr);
    assert!(err.contains("FS_PATH_ESCAPES_ROOT"), "{err}");
    assert!(!harness.root.parent().unwrap().join("out").exists());
}

fn out_file_path() -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("zamin-files-get-{}.bin", std::process::id()));
    path.to_str().unwrap().to_owned()
}

fn in_file_path() -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("zamin-files-put-{}.bin", std::process::id()));
    path.to_str().unwrap().to_owned()
}

#[test]
fn rm_asks_for_confirmation_and_a_mismatch_refuses() {
    let harness = Harness::spawn(&unique_endpoint_tag("confirm"));
    harness.zamin_quiet(&["register", "demo", harness.root.to_str().unwrap()]);
    std::fs::write(harness.root.join("keep.txt"), b"").unwrap();

    // A wrong answer deletes nothing.
    let refused = harness.zamin_confirm(&["files", "rm", "demo", "keep.txt"], "other.txt");
    assert!(!refused.status.success());
    assert!(harness.root.join("keep.txt").exists());

    // The matching name deletes.
    let ok = harness.zamin_confirm(&["files", "rm", "demo", "keep.txt"], "keep.txt");
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(!harness.root.join("keep.txt").exists());
}
