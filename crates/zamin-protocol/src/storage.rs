//! Storage on the wire: where the daemon keeps its data, and the
//! operator-controlled redirect that moves the bulk of it off the OS's
//! default drive. The pointer file lives OUTSIDE the data dir (the
//! platform's own config location — a fixed address the daemon can read
//! before it knows where the data is), holds one absolute path, and is
//! written atomically. Changing it never moves existing data and never
//! applies mid-session: the daemon resolves it once at startup, and
//! `storage.set` says so with `restartRequired: true`.

use serde::{Deserialize, Serialize};

/// `storage.get` — the honest map of where everything lives today.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageGetResult {
    /// The data root the daemon is running on RIGHT NOW (absolute).
    pub root: String,
    /// The platform's default root (what runs when no redirect exists).
    /// Windows: `%LOCALAPPDATA%\\ZIM`; Linux: `$XDG_DATA_HOME/zim` or
    /// `~/.local/share/zim`.
    pub default_root: String,
    /// True when a redirect file is steering the daemon off the default —
    /// the operator (or a launcher's `--data-dir`) chose this root.
    pub redirect_active: bool,
    /// The redirect file's own absolute path (the fixed address).
    pub redirect_path: String,
    /// The pending redirect when one is stored but NOT yet in effect
    /// (written after this daemon started): the root the NEXT daemon
    /// start will use. Absent when nothing pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_root: Option<String>,
    /// `<root>/instances` — the created servers' roots
    /// (`<root>/instances/<serverId>/`). `<root>/servers` is the daemon's
    /// private runtime state, never a server root, and is deliberately
    /// not listed here.
    pub instances_dir: String,
    /// `<root>/cache` — the shared, digest-verified artifact cache.
    pub cache_dir: String,
    /// `<root>/java` — the daemon's fetched JDK runtimes.
    pub java_dir: String,
}

/// `storage.set {root}` — validate, persist the redirect, answer the new
/// map. The daemon does NOT restart itself and does NOT move data: the
/// answer carries `restartRequired` (always true today) and the operator
/// decides what to do with the old root. A relative path, an empty
/// string, a path that is not a directory (and cannot be created), or a
/// directory the daemon cannot write to is a typed refusal — the same
/// loud-validation law discovery roots follow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageSetParams {
    /// The new data root, absolute.
    pub root: String,
}

/// `storage.set` answer: the storage map as it will be after the next
/// daemon start, plus what the operator still owes (a restart; and, if
/// they want the old servers, moving them — the daemon never guesses).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageSetResult {
    /// The root that will be in effect after the restart.
    pub root: String,
    /// Always true today: the running daemon keeps its own root until it
    /// exits. Stated, never implied.
    pub restart_required: bool,
    /// The root the running daemon is still on until that restart.
    pub current_root: String,
    /// The number of registered servers whose files stay in the OLD root
    /// (the daemon does not move them). Zero on a fresh install — the
    /// clean-switch case the setting exists for.
    pub servers_in_current_root: u64,
}
