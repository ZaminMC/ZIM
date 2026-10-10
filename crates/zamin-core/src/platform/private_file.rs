//! Credential-adjacent file writes (ADR-0008): the agent's token and TLS
//! key files must not be world-readable. The permission story is the
//! platform seam's: POSIX needs an explicit 0600; Windows relies on the
//! per-user profile's default ACLs, so the write itself is plain.

use std::io;
use std::path::Path;

/// Write `contents` to `path` private-by-default and ATOMICALLY.
///
/// The write rides a unique temp sibling created with mode 0600 at open
/// time (POSIX), fsynced, then renamed over the target. That closes three
/// holes the old write-then-chmod shape carried: (1) no window in which
/// the secret exists world-readable, (2) a crash mid-write can never
/// leave a truncated secret under its final name, (3) `create_new`
/// (O_EXCL) refuses a pre-existing name — including a pre-planted
/// symlink — so the write can never be redirected through one. Windows
/// relies on the per-user profile's default ACLs for privacy; the
/// atomicity and symlink refusal still apply.
pub fn write_private_file(path: &Path, contents: &str) -> io::Result<()> {
    use std::ffi::OsString;
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut name = path
        .file_name()
        .map_or_else(|| OsString::from("zamin-secret"), |n| n.to_os_string());
    name.push(format!(".tmp-{}", uuid::Uuid::now_v7().simple()));
    let tmp = path.with_file_name(name);

    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    let written = file
        .write_all(contents.as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(source) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(source);
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_written_file_is_private_on_posix() {
        // Unique tag per test: the two tests here share one process id,
        // and cargo runs them in parallel — a shared dir meant one test's
        // cleanup deleted the other's floor mid-run (the ubuntu flake).
        let dir = std::env::temp_dir().join(format!("zim-pf-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.token");
        write_private_file(&path, "payload").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "payload");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_contents_land_everywhere() {
        let dir = std::env::temp_dir().join(format!("zim-pf-contents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.token");
        write_private_file(&path, "payload").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "payload");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
