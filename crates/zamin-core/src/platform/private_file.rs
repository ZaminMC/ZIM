//! Credential-adjacent file writes (ADR-0008): the agent's token and TLS
//! key files must not be world-readable. The permission story is the
//! platform seam's: POSIX needs an explicit 0600; Windows relies on the
//! per-user profile's default ACLs, so the write itself is plain.

use std::io;
use std::path::Path;

/// Write `contents` to `path` with private-by-default permissions. The
/// file is created or truncated, then (on POSIX) tightened to 0600 before
/// the call returns.
pub fn write_private_file(path: &Path, contents: &str) -> io::Result<()> {
    std::fs::write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
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
        let dir = std::env::temp_dir().join(format!(
            "zamin-private-file-private-{}",
            std::process::id()
        ));
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
        let dir = std::env::temp_dir().join(format!(
            "zamin-private-file-contents-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.token");
        write_private_file(&path, "payload").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "payload");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
