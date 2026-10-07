//! Tests for the publish core: §41 selection rules and the walk, the
//! §42 diff and the transactional publication record, the §44/§46
//! scanner's detectors and reviews, the packaging determinism and
//! crash-safety, and the provider/credential contracts.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use zamin_protocol::publish::{
    DiffCounts, FileDiffStatus, ReviewEntry, SecretSeverity, SelectionRule,
};

use super::credentials;
use super::diff::diff_publication;
use super::package::{build_package, PackageOptions, PublishManifest, MANIFEST_FILE_NAME};
use super::provider::{provider_by_id, ProviderPackageRef};
use super::secrets::{blocking, scan, ScanContext, LINE_SCAN_MAX_BYTES};
use super::selection::{
    glob_match, resolve_selection, validate_selection, ResolveLimits, ResolvedFile,
};
use super::state::{
    load_publication, load_reviews, save_publication, save_reviews, PublicationFile,
    PublicationRecord, PUBLICATION_SCHEMA_VERSION,
};
use crate::error::CoreError;
use crate::server::registry::tempdir;

fn rule_folder(path: &str) -> SelectionRule {
    SelectionRule::Folder {
        path: path.to_owned(),
    }
}

fn rule_file(path: &str) -> SelectionRule {
    SelectionRule::File {
        path: path.to_owned(),
    }
}

fn rule_glob(pattern: &str) -> SelectionRule {
    SelectionRule::Glob {
        pattern: pattern.to_owned(),
    }
}

fn selection(
    includes: Vec<SelectionRule>,
    excludes: Vec<SelectionRule>,
) -> zamin_protocol::publish::PublishSelection {
    zamin_protocol::publish::PublishSelection { includes, excludes }
}

/// A believable little server: configs, a plugin tree, a DiscordSRV dir,
/// an env file, and a log.
fn seed_server(root: &Path) {
    fs::create_dir_all(root.join("plugins/TAB")).unwrap();
    fs::create_dir_all(root.join("plugins/DiscordSRV")).unwrap();
    fs::create_dir_all(root.join("plugins/example")).unwrap();
    fs::create_dir_all(root.join("logs")).unwrap();
    fs::write(
        root.join("server.properties"),
        "motd=Hello\nserver-port=25565\n",
    )
    .unwrap();
    fs::write(
        root.join("plugins/TAB/config.yml"),
        "tablist:\n  enabled: true\n",
    )
    .unwrap();
    fs::write(
        root.join("plugins/DiscordSRV/config.yml"),
        "BotToken: \"MTE0MTQxNDE0MTQxNDE0MTQxNA.GhUiWh.SFLQuN8SxjX0COoNUbMQhPdwOOms0TbYmGo5Qu4\"\n",
    )
    .unwrap();
    fs::write(root.join("plugins/example/config.yml"), "spam: eggs\n").unwrap();
    fs::write(root.join(".env"), "DB_PASSWORD=hunter2\n").unwrap();
    fs::write(root.join("logs/latest.log"), "Done!\n").unwrap();
}

// ---------------------------------------------------------------------
// §41 selection: validation, matching, the walk
// ---------------------------------------------------------------------

#[test]
fn selection_validation_refuses_escaping_and_malformed_rules() {
    let ok = selection(
        vec![rule_folder("plugins/TAB"), rule_file("server.properties/")],
        vec![rule_file("server.properties")],
    );
    assert!(
        validate_selection(&ok).is_ok(),
        "trailing slashes normalize away; they are habit, not malice"
    );

    for bad in [
        selection(vec![rule_folder("/absolute")], vec![]),
        selection(vec![rule_file("a\\b.yml")], vec![]),
        selection(vec![rule_glob("../escape")], vec![]),
        selection(vec![rule_file("")], vec![]),
        selection(vec![rule_glob("plugins//x.yml")], vec![]),
        selection(vec![rule_glob("plugins/**x*/y")], vec![]),
    ] {
        assert!(validate_selection(&bad).is_err(), "{bad:?} must be refused");
    }

    let mut many_includes = Vec::new();
    let mut many_excludes = Vec::new();
    for i in 0..40 {
        many_includes.push(rule_file(&format!("a{i}")));
        many_excludes.push(rule_file(&format!("b{i}")));
    }
    assert!(validate_selection(&selection(many_includes, many_excludes)).is_err());
}

#[test]
fn glob_matching_spans_segments_but_star_does_not() {
    assert!(glob_match("plugins/**/*.yml", "plugins/TAB/config.yml"));
    assert!(
        glob_match("plugins/**/*.yml", "plugins/config.yml"),
        "`**` matches zero segments"
    );
    assert!(!glob_match("plugins/**/*.yml", "plugins/TAB/config.json"));
    assert!(
        !glob_match("plugins/*/*.yml", "plugins/a/b/c.yml"),
        "`*` stays in one segment"
    );
    assert!(glob_match("plugins/*/*.yml", "plugins/a/c.yml"));
    assert!(glob_match("server.*", "server.properties"));
    assert!(!glob_match("server.*", "world/level.dat"));
    assert!(glob_match("**/*.jar", "plugins/deep/nested/x.jar"));
}

#[test]
fn resolve_walks_and_hashes_with_excludes_winning() {
    let guard = tempdir::scoped("publish-resolve");
    let root = guard.path.clone();
    seed_server(&root);
    let sel = selection(
        vec![rule_folder("plugins"), rule_file("server.properties")],
        vec![rule_file("plugins/example/config.yml")],
    );
    let resolved = resolve_selection(&root, &sel, ResolveLimits::default()).unwrap();
    assert_eq!(
        resolved.len(),
        3,
        "TAB + DiscordSRV configs + server.properties; example excluded"
    );
    assert!(resolved.contains_key("plugins/TAB/config.yml"));
    assert!(resolved.contains_key("plugins/DiscordSRV/config.yml"));
    assert!(!resolved.contains_key("plugins/example/config.yml"));
    assert!(resolved.contains_key("server.properties"));
    assert!(
        !resolved.contains_key(".env"),
        "nothing outside the include rules is picked"
    );

    let tab = &resolved["plugins/TAB/config.yml"];
    let on_disk = fs::read(&tab.abs).unwrap();
    let expect = {
        use sha2::{Digest, Sha512};
        let mut h = Sha512::new();
        h.update(&on_disk);
        h.finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_eq!(tab.sha512, expect);
    assert_eq!(tab.size as usize, on_disk.len());
}

#[test]
fn empty_include_list_selects_nothing_and_symlinks_are_skipped() {
    let guard = tempdir::scoped("publish-symlink");
    let root = guard.path.clone();
    seed_server(&root);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            root.join("server.properties"),
            root.join("linked.properties"),
        )
        .unwrap();
        std::os::unix::fs::symlink("/etc", root.join("etc-link")).unwrap();
    }
    let resolved =
        resolve_selection(&root, &selection(vec![], vec![]), ResolveLimits::default()).unwrap();
    assert!(
        resolved.is_empty(),
        "empty includes = nothing; the whole root is never packaged"
    );

    let everything = selection(vec![rule_glob("**/*")], vec![rule_folder("logs")]);
    let resolved = resolve_selection(&root, &everything, ResolveLimits::default()).unwrap();
    assert!(!resolved.contains_key("logs/latest.log"));
    assert!(
        !resolved.contains_key("linked.properties"),
        "symlinks never get packaged"
    );
    assert!(!resolved.contains_key("etc-link"));
    assert!(resolved.contains_key("server.properties"));
}

#[test]
fn walk_refuses_to_exceed_its_limits() {
    let guard = tempdir::scoped("publish-limits");
    let root = guard.path.clone();
    seed_server(&root);
    let sel = selection(vec![rule_glob("**/*")], vec![]);
    let err = resolve_selection(
        &root,
        &sel,
        ResolveLimits {
            max_entries: 3,
            max_total_bytes: 1 << 30,
        },
    )
    .unwrap_err();
    assert!(matches!(err, CoreError::PublishTooLarge { .. }));
    let err = resolve_selection(
        &root,
        &sel,
        ResolveLimits {
            max_entries: 10_000,
            max_total_bytes: 16,
        },
    )
    .unwrap_err();
    assert!(matches!(err, CoreError::PublishTooLarge { .. }));
}

// ---------------------------------------------------------------------
// §42 publication state + diff
// ---------------------------------------------------------------------

fn record_with(files: &[(&str, &str, u64)]) -> PublicationRecord {
    PublicationRecord {
        schema_version: PUBLICATION_SCHEMA_VERSION,
        published_at_ms: 1_000,
        title: "Box Demo".into(),
        version: Some("1.0.0".into()),
        changelog: None,
        provider_id: "archive".into(),
        receipt: zamin_protocol::publish::UploadReceipt {
            provider_id: "archive".into(),
            reference: "sha512:abcd1234ef56".into(),
            detail: None,
            at_ms: 1_000,
        },
        package_sha512: "abcd".repeat(32),
        package_bytes: 100,
        files: files
            .iter()
            .map(|(p, sha, size)| {
                (
                    p.to_string(),
                    PublicationFile {
                        sha512: sha.to_string(),
                        size: *size,
                    },
                )
            })
            .collect(),
    }
}

#[test]
fn publication_record_round_trips_and_corruption_is_loud() {
    let guard = tempdir::scoped("publish-state");
    let path = guard.path.join("publication.json");
    assert!(
        load_publication(&path).unwrap().is_none(),
        "absent = never published"
    );

    let record = record_with(&[("server.properties", "aa", 10)]);
    save_publication(&path, &record).unwrap();
    let loaded = load_publication(&path).unwrap().unwrap();
    assert_eq!(loaded, record);

    fs::write(&path, "{ not json").unwrap();
    assert!(
        load_publication(&path).is_err(),
        "a torn/corrupt record is a typed error, never a silent reset"
    );

    let mut wrong_schema = record.clone();
    wrong_schema.schema_version = 99;
    save_publication(&path, &wrong_schema).unwrap();
    assert!(load_publication(&path).is_err());
}

#[test]
fn reviews_round_trip() {
    let guard = tempdir::scoped("publish-reviews");
    let path = guard.path.join("scan-reviews.json");
    assert!(load_reviews(&path).unwrap().is_empty());
    let mut reviews = BTreeSet::new();
    reviews.insert(ReviewEntry {
        file: "plugins/TAB/config.yml".into(),
        kind: "config-secret-key".into(),
    });
    save_reviews(&path, &reviews).unwrap();
    assert_eq!(load_reviews(&path).unwrap(), reviews);
}

#[test]
fn diff_classifies_added_modified_removed_unchanged() {
    let previous = record_with(&[
        ("kept.yml", "sha-kept", 10),
        ("changed.yml", "sha-old", 20),
        ("deleted.yml", "sha-gone", 30),
    ])
    .files;

    let current: BTreeMap<String, ResolvedFile> = [
        ("added.yml", "sha-new", 5u64),
        ("changed.yml", "sha-new-2", 25),
        ("kept.yml", "sha-kept", 10),
    ]
    .iter()
    .map(|(p, sha, size)| {
        (
            p.to_string(),
            ResolvedFile {
                path: p.to_string(),
                abs: std::path::PathBuf::from(p),
                sha512: sha.to_string(),
                size: *size,
            },
        )
    })
    .collect();

    let (files, counts) = diff_publication(&previous, &current);
    assert_eq!(
        counts,
        DiffCounts {
            added: 1,
            modified: 1,
            removed: 1,
            unchanged: 1,
            changed: 3,
        }
    );
    let status = |p: &str| files.iter().find(|f| f.path == p).unwrap().status;
    assert_eq!(status("added.yml"), FileDiffStatus::Added);
    assert_eq!(status("changed.yml"), FileDiffStatus::Modified);
    assert_eq!(status("kept.yml"), FileDiffStatus::Unchanged);
    let deleted = files.iter().find(|f| f.path == "deleted.yml").unwrap();
    assert_eq!(deleted.status, FileDiffStatus::Removed);
    assert_eq!(
        deleted.size,
        Some(30),
        "a removed row keeps its published size"
    );
    assert_eq!(deleted.sha512, None, "a removed row has no current digest");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["added.yml", "changed.yml", "deleted.yml", "kept.yml"]
    );
}

// ---------------------------------------------------------------------
// §44/§46 the scanner
// ---------------------------------------------------------------------

const DISCORD_TOKEN: &str =
    "MTE0MTQxNDE0MTQxNDE0MTQxNA.GhUiWh.SFLQuN8SxjX0COoNUbMQhPdwOOms0TbYmGo5Qu4";

fn resolved_of(root: &Path) -> BTreeMap<String, ResolvedFile> {
    resolve_selection(
        root,
        &selection(vec![rule_glob("**/*")], vec![]),
        ResolveLimits::default(),
    )
    .unwrap()
}

fn kinds_of<'a>(
    report: &'a zamin_protocol::publish::ScanReport,
    file: &str,
) -> Vec<(&'a str, SecretSeverity)> {
    report
        .findings
        .iter()
        .filter(|f| f.file == file)
        .map(|f| (f.kind.as_str(), f.severity))
        .collect()
}

#[test]
fn scanner_finds_the_discord_bot_token_and_escalates_inside_discordsrv() {
    let guard = tempdir::scoped("publish-scan-dsrv");
    let root = guard.path.clone();
    seed_server(&root);
    let resolved = resolved_of(&root);
    let report = scan(&resolved, &ScanContext::default(), &BTreeSet::new());

    let dsrv = kinds_of(&report, "plugins/DiscordSRV/config.yml");
    assert!(
        dsrv.contains(&("discord-bot-token", SecretSeverity::Critical)),
        "token detection inside DiscordSRV is critical: {dsrv:?}"
    );
    assert!(
        dsrv.contains(&("config-secret-key", SecretSeverity::Critical)),
        "the BotToken config key is also caught, critical inside DiscordSRV: {dsrv:?}"
    );
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.excerpt.contains(DISCORD_TOKEN)),
        "the excerpt must be redacted, never the token itself"
    );

    // The same token outside DiscordSRV is high, not critical.
    let guard2 = tempdir::scoped("publish-scan-other");
    let root2 = guard2.path.clone();
    fs::create_dir_all(root2.join("plugins/Other")).unwrap();
    fs::write(
        root2.join("plugins/Other/config.yml"),
        format!("token: \"{DISCORD_TOKEN}\"\n"),
    )
    .unwrap();
    let report2 = scan(
        &resolved_of(&root2),
        &ScanContext::default(),
        &BTreeSet::new(),
    );
    let other = kinds_of(&report2, "plugins/Other/config.yml");
    assert!(
        other.contains(&("discord-bot-token", SecretSeverity::High)),
        "{other:?}"
    );
}

#[test]
fn scanner_advises_on_discordsrv_config_only_when_the_plugin_is_loaded() {
    let guard = tempdir::scoped("publish-scan-advisory");
    let root = guard.path.clone();
    seed_server(&root);
    let resolved = resolved_of(&root);

    let idle = scan(&resolved, &ScanContext::default(), &BTreeSet::new());
    assert!(
        !idle.findings.iter().any(|f| f.kind == "discord-srv-config"),
        "no advisory while the server is not known to be running DiscordSRV"
    );

    let live = scan(
        &resolved,
        &ScanContext {
            discord_srv_active: true,
        },
        &BTreeSet::new(),
    );
    let advisory = live
        .findings
        .iter()
        .find(|f| f.kind == "discord-srv-config")
        .expect("the §44 advisory fires when DiscordSRV is loaded");
    assert_eq!(advisory.severity, SecretSeverity::Critical);
    assert_eq!(advisory.line, 0, "file-level findings ride line 0");
}

#[test]
fn scanner_token_detectors() {
    let guard = tempdir::scoped("publish-scan-tokens");
    let root = guard.path.clone();
    let body = "aws: AKIAIOSFODNN7EXAMPLE\ngithub: ghp_abcdefghijklmnopqrstuvwxyz0123456789\nslack: xoxb-123456789012-1234567890123-abcdefghijklmnopqrstuvwx\nstripe: sk_live_abcdefghijklmnop123456\ngoogle: AIzaSyA1bC2dE3fG4hI5jK6lM7nO8pQ9rS0tU1v\njwt: eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefGHIJKLMNOPqrstuvwxyz0123456789\nwebhook: https://discord.com/api/webhooks/1234567890/abcdefghijklmnop\npem: -----BEGIN RSA PRIVATE KEY-----\n";
    fs::write(root.join("secrets.txt"), body).unwrap();
    let report = scan(
        &resolved_of(&root),
        &ScanContext::default(),
        &BTreeSet::new(),
    );
    let kinds: Vec<&str> = report.findings.iter().map(|f| f.kind.as_str()).collect();
    for expected in [
        "aws-access-key",
        "github-token",
        "slack-token",
        "stripe-secret-key",
        "google-api-key",
        "jwt",
        "webhook-url",
        "private-key",
    ] {
        assert!(
            kinds.contains(&expected),
            "expected {expected} in {kinds:?}"
        );
    }
}

#[test]
fn scanner_config_key_heuristic_skips_placeholders_and_pointers() {
    let guard = tempdir::scoped("publish-scan-configkey");
    let root = guard.path.clone();
    fs::write(
        root.join("config.yml"),
        "password: hunter2\ntoken: ${ENV_TOKEN}\napi-key: 12345\ndb-password: \"\"\ntoken-file: secrets.txt\nDatabasePasswordPath: /run/secrets/db\ntoken: true\nretries: 3\nsecret: changeme\nToken: your-token-here\n",
    )
    .unwrap();
    let report = scan(
        &resolved_of(&root),
        &ScanContext::default(),
        &BTreeSet::new(),
    );
    let keys: Vec<&str> = report
        .findings
        .iter()
        .filter(|f| f.kind == "config-secret-key")
        .map(|f| f.excerpt.as_str())
        .collect();
    assert_eq!(
        keys,
        vec!["key `password`"],
        "only the real value is a finding: {keys:?}"
    );
    assert!(
        report
            .findings
            .iter()
            .all(|f| !f.excerpt.contains("hunter2")),
        "values are never in excerpts"
    );
}

#[test]
fn scanner_sensitive_filenames_and_high_entropy_and_the_review_mechanism() {
    let guard = tempdir::scoped("publish-scan-files");
    let root = guard.path.clone();
    fs::create_dir_all(root.join("keys")).unwrap();
    fs::write(root.join("keys/server.pem"), "not actually a pem\n").unwrap();
    fs::write(
        root.join("keys/noise.txt"),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .unwrap();
    // A real high-entropy mixed run for the entropy detector:
    fs::write(
        root.join("keys/rand.txt"),
        "hQ7mT2xL9pZ3vB8nK1cW4yF6dJ0rS5gU2iA8oE3qN7tX\n",
    )
    .unwrap();
    let resolved = resolved_of(&root);
    let report = scan(&resolved, &ScanContext::default(), &BTreeSet::new());

    let pem = report
        .findings
        .iter()
        .find(|f| f.file == "keys/server.pem")
        .expect("the .pem filename is a finding");
    assert_eq!(pem.kind, "sensitive-filename");
    assert_eq!(pem.severity, SecretSeverity::High);
    assert!(report
        .findings
        .iter()
        .any(|f| f.kind == "high-entropy-string" && f.file == "keys/rand.txt"));
    assert!(
        !report.findings.iter().any(|f| f.file == "keys/noise.txt"),
        "a constant run carries no entropy"
    );
    assert_eq!(
        blocking(&report).len(),
        1,
        "only the sensitive filename blocks; high-entropy is low severity by design"
    );

    // Reviews: mark the filename finding reviewed; it stops blocking but
    // stays visible.
    let mut reviews = BTreeSet::new();
    reviews.insert(ReviewEntry {
        file: "keys/server.pem".into(),
        kind: "sensitive-filename".into(),
    });
    let reviewed = scan(&resolved, &ScanContext::default(), &reviews);
    assert_eq!(
        blocking(&reviewed).len(),
        0,
        "the only blocking finding was reviewed"
    );
    assert!(reviewed
        .findings
        .iter()
        .any(|f| f.file == "keys/server.pem" && f.reviewed));
}

#[test]
fn scanner_counts_skipped_files_instead_of_lying() {
    let guard = tempdir::scoped("publish-scan-skip");
    let root = guard.path.clone();
    fs::write(root.join("big.bin"), vec![0u8; 32]).unwrap();
    let mut resolved = resolved_of(&root);
    let entry = resolved.get_mut("big.bin").unwrap();
    entry.size = LINE_SCAN_MAX_BYTES + 1; // pretend it is huge
    let report = scan(&resolved, &ScanContext::default(), &BTreeSet::new());
    assert_eq!(report.files_skipped, 1);
    assert_eq!(report.files_scanned, 0);
}

// ---------------------------------------------------------------------
// packaging
// ---------------------------------------------------------------------

fn quiet_progress() -> Arc<dyn Fn(super::package::PackageProgress) + Send + Sync> {
    Arc::new(|_| {})
}

#[test]
fn package_is_deterministic_self_describing_and_committed_atomically() {
    let guard = tempdir::scoped("publish-pkg");
    let root = guard.path.join("server");
    seed_server(&root);
    let resolved = resolve_selection(
        &root,
        &selection(vec![rule_folder("plugins")], vec![]),
        ResolveLimits::default(),
    )
    .unwrap();
    let files: Vec<&ResolvedFile> = resolved.values().collect();
    let out = guard.path.join("package.zip");
    let staging = guard.path.join("package.zip.staging");

    let mut opts = PackageOptions {
        files: &files,
        out_path: &out,
        staging_path: &staging,
        title: "Box Demo".into(),
        description: "demo".into(),
        version: Some("1.0.0".into()),
        changelog: Some("first".into()),
        provider_id: "archive".into(),
        created_at_ms: 1_000,
        max_entries: 10_000,
        max_total_bytes: 1 << 30,
        cancel: Arc::new(AtomicBool::new(false)),
        progress: quiet_progress(),
    };
    let outcome = build_package(&mut opts).unwrap();
    assert_eq!(outcome.file_count, 3);
    assert!(out.is_file());
    assert!(
        !staging.exists(),
        "the staging file is gone after the commit rename"
    );

    // Determinism: same inputs, same digest.
    let out2 = guard.path.join("package2.zip");
    let staging2 = guard.path.join("package2.zip.staging");
    let mut opts2 = PackageOptions {
        files: &files,
        out_path: &out2,
        staging_path: &staging2,
        title: "Box Demo".into(),
        description: "demo".into(),
        version: Some("1.0.0".into()),
        changelog: Some("first".into()),
        provider_id: "archive".into(),
        created_at_ms: 1_000,
        max_entries: 10_000,
        max_total_bytes: 1 << 30,
        cancel: Arc::new(AtomicBool::new(false)),
        progress: quiet_progress(),
    };
    let outcome2 = build_package(&mut opts2).unwrap();
    assert_eq!(
        outcome.sha512, outcome2.sha512,
        "identical content must package identically"
    );
    assert_eq!(outcome.size_bytes, outcome2.size_bytes);

    // The manifest rides inside the archive and agrees with the outcome.
    let f = fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(f).unwrap();
    let mut manifest_bytes = Vec::new();
    use std::io::Read;
    zip.by_name(MANIFEST_FILE_NAME)
        .unwrap()
        .read_to_end(&mut manifest_bytes)
        .unwrap();
    let manifest: PublishManifest = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest.file_count, 3);
    assert_eq!(manifest.files.len(), 3);
    assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
    assert_eq!(manifest.files[0].path, "plugins/DiscordSRV/config.yml");
}

#[test]
fn package_honors_cancellation_and_limits() {
    let guard = tempdir::scoped("publish-pkg-cancel");
    let root = guard.path.join("server");
    seed_server(&root);
    let resolved = resolve_selection(
        &root,
        &selection(vec![rule_folder("plugins")], vec![]),
        ResolveLimits::default(),
    )
    .unwrap();
    let files: Vec<&ResolvedFile> = resolved.values().collect();

    // Cancellation before the first file: the staging file is cleaned up
    // and the error is the typed Cancelled.
    let out = guard.path.join("c.zip");
    let staging = guard.path.join("c.zip.staging");
    let cancel = Arc::new(AtomicBool::new(true));
    let mut opts = PackageOptions {
        files: &files,
        out_path: &out,
        staging_path: &staging,
        title: "t".into(),
        description: String::new(),
        version: None,
        changelog: None,
        provider_id: "archive".into(),
        created_at_ms: 1,
        max_entries: 10_000,
        max_total_bytes: 1 << 30,
        cancel: cancel.clone(),
        progress: quiet_progress(),
    };
    let err = build_package(&mut opts).unwrap_err();
    assert!(matches!(err, CoreError::Cancelled));
    assert!(
        !staging.exists(),
        "a cancelled pack leaves no staging litter"
    );

    // Entry cap.
    cancel.store(false, std::sync::atomic::Ordering::Relaxed);
    let out3 = guard.path.join("l.zip");
    let staging3 = guard.path.join("l.zip.staging");
    let mut opts3 = PackageOptions {
        files: &files,
        out_path: &out3,
        staging_path: &staging3,
        title: "t".into(),
        description: String::new(),
        version: None,
        changelog: None,
        provider_id: "archive".into(),
        created_at_ms: 1,
        max_entries: 2,
        max_total_bytes: 1 << 30,
        cancel,
        progress: quiet_progress(),
    };
    let err = build_package(&mut opts3).unwrap_err();
    assert!(matches!(err, CoreError::PublishTooLarge { .. }));
}

// ---------------------------------------------------------------------
// providers + credentials
// ---------------------------------------------------------------------

#[test]
fn archive_provider_receipts_without_uploading() {
    let archive = provider_by_id("archive").unwrap();
    assert!(provider_by_id("nope").is_none());
    assert!(archive.validate_settings(&BTreeMap::new()).is_ok());
    assert!(archive
        .validate_settings(&BTreeMap::from([("outDir".to_owned(), "/x".to_owned())]))
        .is_err());

    let manifest = PublishManifest {
        format_version: 1,
        created_at_ms: 1,
        title: "t".into(),
        description: String::new(),
        version: None,
        changelog: None,
        provider_id: "archive".into(),
        file_count: 1,
        total_bytes: 1,
        files: vec![],
    };
    let pkg = ProviderPackageRef {
        path: Path::new("/unused"),
        sha512: "abcdef1234567890",
        size_bytes: 1,
        manifest: &manifest,
    };
    let receipt = archive.upload(&pkg, &BTreeMap::new(), None).unwrap();
    assert_eq!(receipt.provider_id, "archive");
    assert_eq!(receipt.reference, "sha512:abcdef123456");
    assert!(receipt.detail.is_some());
}

#[test]
fn local_dir_provider_copies_the_package_and_writes_its_receipt() {
    let local = provider_by_id("local-dir").unwrap();
    assert!(
        local.validate_settings(&BTreeMap::new()).is_err(),
        "outDir is required"
    );
    assert!(local
        .validate_settings(&BTreeMap::from([(
            "outDir".to_owned(),
            "relative".to_owned()
        )]))
        .is_err());
    assert!(local
        .validate_settings(&BTreeMap::from([("mystery".to_owned(), "x".to_owned())]))
        .is_err());

    let src_guard = tempdir::scoped("publish-provider-src");
    let src = src_guard.path.join("package.zip");
    fs::write(&src, b"package bytes here").unwrap();
    let out_guard = tempdir::scoped("publish-provider-out");
    let settings = BTreeMap::from([("outDir".to_owned(), out_guard.path.display().to_string())]);
    assert!(local.validate_settings(&settings).is_ok());

    let manifest = PublishManifest {
        format_version: 1,
        created_at_ms: 1,
        title: "t".into(),
        description: String::new(),
        version: None,
        changelog: None,
        provider_id: "local-dir".into(),
        file_count: 1,
        total_bytes: 18,
        files: vec![],
    };
    let sha = "a".repeat(64);
    let pkg = ProviderPackageRef {
        path: &src,
        sha512: &sha,
        size_bytes: 18,
        manifest: &manifest,
    };
    let receipt = local.upload(&pkg, &settings, None).unwrap();
    assert_eq!(receipt.provider_id, "local-dir");
    let dest = out_guard.path.join("package.zip");
    assert_eq!(fs::read(&dest).unwrap(), b"package bytes here");
    let receipt_path = out_guard.path.join("package.receipt.json");
    let stored: zamin_protocol::publish::UploadReceipt =
        serde_json::from_str(&fs::read_to_string(&receipt_path).unwrap()).unwrap();
    assert_eq!(stored, receipt, "the receipt beside the copy matches");
}

#[test]
fn credentials_ride_the_environment_channel_only() {
    assert_eq!(
        credentials::credential_env_var("built-by-bit"),
        "ZAMIN_PUBLISH_CREDENTIAL_BUILT_BY_BIT"
    );
    assert_eq!(
        credentials::credential_env_var("archive"),
        "ZAMIN_PUBLISH_CREDENTIAL_ARCHIVE"
    );
    let env = |name: &str| {
        if name == "ZAMIN_PUBLISH_CREDENTIAL_ARCHIVE" {
            Some("hunter2".to_owned())
        } else {
            None
        }
    };
    assert_eq!(
        credentials::resolve_credential_in("archive", &env).as_deref(),
        Some("hunter2")
    );
    assert_eq!(credentials::resolve_credential_in("local-dir", &env), None);
    let blank = |_name: &str| Some("   ".to_owned());
    assert_eq!(
        credentials::resolve_credential_in("archive", &blank),
        None,
        "a whitespace placeholder is not a credential"
    );
    assert_eq!(
        credentials::redact("hunter2secret"),
        "hunt…[redacted] (len 13)"
    );
    assert_eq!(credentials::redact(""), "(empty)");
}
