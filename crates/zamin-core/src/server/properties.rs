//! `server.properties` — the boot authority's file (ADR-0007: it is
//! Minecraft's file; ZIM writes it exactly twice, and always through
//! this module's one law).
//!
//! 1. At creation, the stamped template's `server-port` is pointed at the
//!    port the operator asked for.
//! 2. At boot (server start), the layered config's desired port — the
//!    Network tab's edit — is re-stamped into the file, so the process
//!    the JVM launches actually binds the port the panel shows. Without
//!    this second stamp a port edit only changed the config model, and
//!    the server kept binding the port its properties file carried from
//!    creation day.

use std::path::Path;

/// Point `server-port` in `root/server.properties` at `port`.
///
/// Idempotent and tolerant: a missing `server-port` line is appended (a
/// stamped template always carries one, but a registered server's
/// hand-written file may not); a missing or unreadable file is left
/// alone — the layered setting stays the authoritative desired value and
/// the preflight surfaces the gap, never a fabricated properties file.
/// The write is atomic (staging + rename), so a crash mid-patch can
/// never leave a truncated properties file behind.
pub fn patch_server_port(root: &Path, port: u16) {
    let path = root.join("server.properties");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let patched = {
        let mut found = false;
        let mut out = String::new();
        for line in content.lines() {
            if line.trim_start().starts_with("server-port=") {
                out.push_str(&format!("server-port={port}\n"));
                found = true;
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        if !found {
            out.push_str(&format!("server-port={port}\n"));
        }
        out
    };
    let _ = crate::fsops::atomic_write(&path, patched.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_the_port_line_in_place_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join("zim-port-patch-basic");
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(
            dir.join("server.properties"),
            "motd=hello\nserver-port=25565\nonline-mode=true\n",
        )
        .unwrap();
        patch_server_port(&dir, 25580);
        let patched = std::fs::read_to_string(dir.join("server.properties")).unwrap();
        assert_eq!(
            patched, "motd=hello\nserver-port=25580\nonline-mode=true\n",
            "only the port line moves"
        );
    }

    #[test]
    fn appends_when_the_file_carries_no_port_line() {
        let dir = std::env::temp_dir().join("zim-port-patch-append");
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(dir.join("server.properties"), "motd=hello\n").unwrap();
        patch_server_port(&dir, 25599);
        let patched = std::fs::read_to_string(dir.join("server.properties")).unwrap();
        assert!(
            patched.contains("motd=hello\n") && patched.contains("server-port=25599\n"),
            "the hand-written file gains the port line: {patched}"
        );
    }

    #[test]
    fn a_missing_properties_file_is_left_absent() {
        let dir = std::env::temp_dir().join("zim-port-patch-absent");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).ok();
        patch_server_port(&dir, 25565);
        assert!(
            !dir.join("server.properties").exists(),
            "the boot patch never invents a properties file"
        );
    }
}
