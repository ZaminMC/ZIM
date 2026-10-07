//! The audit log (ADR-0011's deferred list, landed): an append-only JSONL
//! record of who did what — every session handshake and every mutating
//! command, with the outcome the daemon answered. Reads are not audited
//! (a listing floods the file without making the system safer). The agent
//! gates remote identities before the daemon ever sees them, so the actor
//! recorded here is the protocol client (name + version), honestly — the
//! daemon cannot distinguish a local client from a relayed one, and does
//! not pretend to.
//!
//! Best-effort by policy: a failed append logs a warning and the daemon
//! keeps serving. The audit is evidence, not a choke point.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

/// Methods that change server state, files, or jobs. Everything else is
/// observation and stays out of the log (ADR-0011: mutations only).
pub const AUDITED_METHODS: &[&str] = &[
    zamin_protocol::methods::SERVER_REGISTER,
    zamin_protocol::methods::SERVER_CREATE,
    zamin_protocol::methods::SERVER_UPDATE,
    zamin_protocol::methods::SERVER_REMOVE,
    zamin_protocol::methods::SERVER_START,
    zamin_protocol::methods::SERVER_STOP,
    zamin_protocol::methods::SERVER_RESTART,
    zamin_protocol::methods::SERVER_KILL,
    zamin_protocol::methods::SERVER_STDIN,
    zamin_protocol::methods::BACKUP_CREATE,
    zamin_protocol::methods::BACKUP_RESTORE,
    zamin_protocol::methods::JAVA_INSTALL,
    zamin_protocol::methods::PLUGINS_INSTALL,
    zamin_protocol::methods::PLUGINS_DELETE,
    zamin_protocol::methods::JOBS_CANCEL,
    zamin_protocol::methods::FILES_WRITE,
    zamin_protocol::methods::FILES_COMMIT,
    zamin_protocol::methods::FILES_MKDIR,
    zamin_protocol::methods::FILES_RENAME,
    zamin_protocol::methods::FILES_DELETE,
    zamin_protocol::methods::SCHEDULES_CREATE,
    zamin_protocol::methods::SCHEDULES_UPDATE,
    zamin_protocol::methods::SCHEDULES_DELETE,
];

#[derive(Clone)]
pub struct Audit {
    path: PathBuf,
    /// Serializes appends. Open-per-write: a rotated/deleted file cannot
    /// wedge a long-lived handle, and O_APPEND keeps every entry whole.
    lock: std::sync::Arc<Mutex<()>>,
}

impl Audit {
    pub fn new(data_dir: &Path) -> Audit {
        Audit {
            path: data_dir.join("audit.log"),
            lock: std::sync::Arc::new(Mutex::new(())),
        }
    }

    /// One line of JSONL. `client` is the hello's ClientInfo; `outcome` is
    /// "ok" or the protocol error code the daemon answered with.
    pub fn record(
        &self,
        method: &str,
        server_id: Option<&str>,
        outcome: &str,
        client: Option<(&str, &str)>,
    ) {
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let entry = json!({
            "tsMs": ts_ms,
            "method": method,
            "serverId": server_id,
            "outcome": outcome,
            "client": client.map(|(name, version)| json!({"name": name, "version": version})),
        });

        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| writeln!(file, "{entry}"));
        if let Err(error) = result {
            tracing::warn!("audit append to {:?} failed: {error}", self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zamind-audit-{tag}-{}-{}",
            std::process::id(),
            std::time::Instant::now().elapsed().as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn entries_append_one_json_line_each() {
        let dir = temp_dir("append");
        let audit = Audit::new(&dir);
        audit.record("daemon.hello", None, "ok", Some(("zamin-cli", "0.1.0")));
        audit.record(
            "server.start",
            Some("demo"),
            "ok",
            Some(("zamin-cli", "0.1.0")),
        );
        audit.record("server.stop", Some("demo"), "SERVER_STOP_TIMEOUT", None);

        let content = std::fs::read_to_string(dir.join("audit.log")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["method"], "server.start");
        assert_eq!(second["serverId"], "demo");
        assert_eq!(second["outcome"], "ok");
        assert_eq!(second["client"]["name"], "zamin-cli");
        assert!(second["tsMs"].as_u64().unwrap() > 0);

        let third: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(third["outcome"], "SERVER_STOP_TIMEOUT");
        assert!(third["client"].is_null());
    }

    #[test]
    fn clones_share_one_append_order() {
        let dir = temp_dir("clone");
        let audit = Audit::new(&dir);
        let audit2 = audit.clone();
        audit.record("server.start", Some("a"), "ok", None);
        audit2.record("server.stop", Some("a"), "ok", None);
        let content = std::fs::read_to_string(dir.join("audit.log")).unwrap();
        // Parse once and borrow from the parsed values (the lines own the
        // data their borrows point into).
        let lines: Vec<serde_json::Value> = content
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let methods: Vec<&str> = lines
            .iter()
            .map(|value| value["method"].as_str().unwrap())
            .collect();
        assert_eq!(methods, ["server.start", "server.stop"]);
    }

    #[test]
    fn a_missing_directory_is_a_warning_not_a_panic() {
        let dir = temp_dir("gone");
        let audit = Audit::new(&dir.join("never-created"));
        // Must not panic; the warning is tracing's to carry.
        audit.record("server.start", Some("x"), "ok", None);
    }
}
