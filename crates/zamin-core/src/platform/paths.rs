//! Platform application directories (ADR-0008). Pure functions take the
//! environment as input so tests never mutate process state.

use std::path::{Path, PathBuf};

/// Daemon-owned state: registry, per-server state, logs of the daemon
/// itself (ADR-0004). Windows: `%LOCALAPPDATA%\ZIM`. Linux:
/// `$XDG_DATA_HOME/zim`, default `~/.local/share/zim`.
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
        data_dir_windows(&base)
    }
    #[cfg(unix)]
    {
        let xdg = std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|s| !s.is_empty());
        let home = std::env::var("HOME").ok().filter(|s| !s.is_empty());
        data_dir_unix(xdg.as_deref(), home.as_deref().map(PathBuf::from))
    }
}

/// Human-edited configuration (ADR-0007). Windows: same tree as data.
/// Linux: `$XDG_CONFIG_HOME/zim`, default `~/.config/zim`.
pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        data_dir()
    }
    #[cfg(unix)]
    {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| !s.is_empty());
        let home = std::env::var("HOME").ok().filter(|s| !s.is_empty());
        config_dir_unix(xdg.as_deref(), home.as_deref().map(PathBuf::from))
    }
}

/// Runtime artifacts (sockets, lock-free pid files). Linux:
/// `$XDG_RUNTIME_DIR`, default `/run/user/<uid>` — same convention the
/// IPC endpoint uses.
#[cfg(unix)]
pub fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let uid = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok()))
        })
        .unwrap_or(0);
    PathBuf::from(format!("/run/user/{uid}"))
}

#[cfg(windows)]
fn data_dir_windows(local_app_data: &str) -> PathBuf {
    if local_app_data.is_empty() {
        // No LOCALAPPDATA (rare, broken profile): fall next to the exe
        // rather than failing; the daemon still needs somewhere to write.
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(local_app_data).join("ZIM")
    }
}

#[cfg(unix)]
fn data_dir_unix(xdg_data_home: Option<&str>, home: Option<PathBuf>) -> PathBuf {
    match xdg_data_home {
        Some(dir) => PathBuf::from(dir).join("zim"),
        None => match home {
            Some(home) => home.join(".local/share/zim"),
            None => PathBuf::from(".local/share/zim"),
        },
    }
}

#[cfg(unix)]
fn config_dir_unix(xdg_config_home: Option<&str>, home: Option<PathBuf>) -> PathBuf {
    match xdg_config_home {
        Some(dir) => PathBuf::from(dir).join("zim"),
        None => match home {
            Some(home) => home.join(".config/zim"),
            None => PathBuf::from(".config/zim"),
        },
    }
}

// --- the storage redirect (user-controlled data root) ------------------
//
// The data bulk — server instances, the artifact cache, fetched JDKs —
// lands under one root the OPERATOR chooses, because the OS's default
// drive is not always the right one (a small C: SSD and a large D: is
// the classic machine). The pointer must live at an address findable
// WITHOUT knowing the data dir, so it rides the platform's own config
// location: one line, one absolute path, written atomically. The daemon
// resolves it exactly once, at startup.

/// The redirect pointer file's fixed address (inside the platform config
/// dir — deliberately NOT inside the data dir, or moving the data would
/// move the pointer that says where the data is).
pub fn storage_redirect_file() -> PathBuf {
    config_dir().join("storage-location.txt")
}

/// The startup resolution verdict, stated so the daemon can log it.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDataDir {
    /// The root this daemon run uses.
    pub root: PathBuf,
    /// True when a usable redirect named the root (an explicit flag is
    /// its own, stronger authority and does not count as a redirect).
    pub redirected: bool,
    /// True when a redirect file existed but its content was unusable
    /// (empty or relative): the platform default ran anyway, and the
    /// daemon should say so — a silently ignored preference is a lie.
    pub redirect_ignored: bool,
}

/// The pure resolution core: `explicit` (a launcher's `--data-dir`)
/// always wins; else the trimmed redirect text must name an ABSOLUTE
/// path; else the platform default. Testable without touching process
/// state (the file-reading wrapper below is the only I/O).
pub fn resolve_data_dir_from(
    explicit: Option<&Path>,
    redirect_text: Option<&str>,
    platform_default: &Path,
) -> ResolvedDataDir {
    if let Some(explicit) = explicit {
        return ResolvedDataDir {
            root: explicit.to_path_buf(),
            redirected: false,
            redirect_ignored: false,
        };
    }
    let trimmed = redirect_text.map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        Some(text) if Path::new(text).is_absolute() => ResolvedDataDir {
            root: PathBuf::from(text),
            redirected: true,
            redirect_ignored: false,
        },
        Some(_) => ResolvedDataDir {
            root: platform_default.to_path_buf(),
            redirected: false,
            redirect_ignored: true,
        },
        None => ResolvedDataDir {
            root: platform_default.to_path_buf(),
            redirected: false,
            redirect_ignored: false,
        },
    }
}

/// The I/O wrapper the daemon's main calls once at startup: read the
/// redirect file (absent = no redirect), resolve, done.
pub fn resolve_data_dir(explicit: Option<PathBuf>) -> ResolvedDataDir {
    let redirect_file = storage_redirect_file();
    let text = std::fs::read_to_string(&redirect_file).ok();
    resolve_data_dir_from(explicit.as_deref(), text.as_deref(), &data_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn unix_dirs_follow_xdg() {
        let data = data_dir_unix(Some("/xdg/data"), Some("/home/u".into()));
        assert_eq!(data, PathBuf::from("/xdg/data/zim"));

        let config = config_dir_unix(Some("/xdg/conf"), Some("/home/u".into()));
        assert_eq!(config, PathBuf::from("/xdg/conf/zim"));

        let data_default = data_dir_unix(None, Some("/home/u".into()));
        assert_eq!(data_default, PathBuf::from("/home/u/.local/share/zim"));

        let config_default = config_dir_unix(None, None);
        assert_eq!(config_default, PathBuf::from(".config/zim"));
    }

    #[test]
    #[cfg(windows)]
    fn windows_dir_uses_local_app_data() {
        let dir = data_dir_windows(r"C:\Users\u\AppData\Local");
        assert_eq!(dir, PathBuf::from(r"C:\Users\u\AppData\Local\ZIM"));
        let fallback = data_dir_windows("");
        assert!(!fallback.as_os_str().is_empty());
    }

    // -- the storage redirect's resolution laws --------------------------

    #[test]
    fn an_explicit_flag_beats_the_redirect() {
        let verdict = resolve_data_dir_from(
            Some(Path::new("/opt/zam-data")),
            Some("/d/ZIM"),
            Path::new("/home/u/.local/share/zim"),
        );
        assert_eq!(verdict.root, PathBuf::from("/opt/zam-data"));
        assert!(!verdict.redirected);
        assert!(!verdict.redirect_ignored);
    }

    #[test]
    #[cfg(unix)] // "/d/ZIM" is absolute only from Unix's point of view —
                 // on Windows it lacks a drive, so the Windows half of the law lives
                 // in the test below.
    fn an_absolute_redirect_wins_over_the_default() {
        let verdict = resolve_data_dir_from(
            None,
            Some("  /d/ZIM  \n"),
            Path::new("/home/u/.local/share/zim"),
        );
        assert_eq!(verdict.root, PathBuf::from("/d/ZIM"));
        assert!(verdict.redirected);
        assert!(!verdict.redirect_ignored);
    }

    #[test]
    #[cfg(windows)] // the same text, from Windows' point of view: no
                    // drive, no absoluteness — the redirect is ignored and says so.
    fn a_driveless_redirect_is_relative_on_windows_and_ignored() {
        let verdict = resolve_data_dir_from(
            None,
            Some("  /d/ZIM  \n"),
            Path::new(r"C:\Users\u\AppData\Local\ZIM"),
        );
        assert_eq!(verdict.root, PathBuf::from(r"C:\Users\u\AppData\Local\ZIM"));
        assert!(!verdict.redirected);
        assert!(verdict.redirect_ignored);
    }

    #[test]
    #[cfg(unix)] // "D:\ZIM" is relative only from Unix's point of view
    fn a_relative_redirect_is_ignored_and_says_so() {
        let verdict =
            resolve_data_dir_from(None, Some("D:\\ZIM"), Path::new("/home/u/.local/share/zim"));
        assert_eq!(verdict.root, PathBuf::from("/home/u/.local/share/zim"));
        assert!(!verdict.redirected);
        assert!(verdict.redirect_ignored);
    }

    #[test]
    fn no_redirect_is_the_plain_default() {
        let verdict = resolve_data_dir_from(None, None, Path::new("/home/u/.local/share/zim"));
        assert_eq!(verdict.root, PathBuf::from("/home/u/.local/share/zim"));
        assert!(!verdict.redirected);
        assert!(!verdict.redirect_ignored);
    }

    #[test]
    fn whitespace_only_redirect_is_absent_for_all_intents() {
        let verdict = resolve_data_dir_from(None, Some("   \n"), Path::new("/default"));
        assert_eq!(verdict.root, PathBuf::from("/default"));
        assert!(!verdict.redirected);
        // Empty after trim: nothing usable existed, so nothing was ignored.
        assert!(!verdict.redirect_ignored);
    }

    #[test]
    #[cfg(windows)]
    fn a_windows_absolute_redirect_resolves() {
        let verdict = resolve_data_dir_from(
            None,
            Some(r"D:\ZIM"),
            Path::new(r"C:\Users\u\AppData\Local\ZIM"),
        );
        assert_eq!(verdict.root, PathBuf::from(r"D:\ZIM"));
        assert!(verdict.redirected);
    }
}
