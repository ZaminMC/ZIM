// The tests (ADR-0008: platform-conditional assertions live in
// files named tests.rs — the seam guard's test exemption).

use super::*;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zamin-agent-tls-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn generated_once_fingerprint_stable() {
    let dir = temp_dir("stable");
    let first = TlsMaterial::load_or_generate(&dir).expect("generate");
    let second = TlsMaterial::load_or_generate(&dir).expect("reload");
    assert_eq!(first.fingerprint_hex(), second.fingerprint_hex());
    assert_eq!(first.fingerprint_hex().len(), 64);
    // A second install in a fresh dir yields a different identity.
    let other = TlsMaterial::load_or_generate(&temp_dir("other")).expect("other");
    assert_ne!(first.fingerprint_hex(), other.fingerprint_hex());
}

#[cfg(unix)]
#[test]
fn key_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("perms");
    let _ = TlsMaterial::load_or_generate(&dir).expect("generate");
    let mode = fs::metadata(dir.join(KEY_FILE))
        .expect("key file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn server_config_builds_and_display_groups() {
    let dir = temp_dir("config");
    let material = TlsMaterial::load_or_generate(&dir).expect("generate");
    material.server_config().expect("server config");
    let hex = material.fingerprint_hex();
    let display = fingerprint_display(&hex);
    assert_eq!(display.split(':').count(), 32);
    assert_eq!(display.replace(':', ""), hex);
}

#[test]
fn pem_round_trips_through_the_loader() {
    let dir = temp_dir("pem");
    let material = TlsMaterial::load_or_generate(&dir).expect("generate");
    let reloaded = TlsMaterial::load_or_generate(&dir).expect("reload");
    assert_eq!(material.cert_pem(), reloaded.cert_pem());
    assert_eq!(material.fingerprint_hex(), reloaded.fingerprint_hex());
}
