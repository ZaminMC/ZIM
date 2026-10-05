//! Platform application directories (ADR-0008). Pure functions take the
//! environment as input so tests never mutate process state.

use std::path::PathBuf;

/// Daemon-owned state: registry, per-server state, logs of the daemon
/// itself (ADR-0004). Windows: `%LOCALAPPDATA%\ZaminPanel`. Linux:
/// `$XDG_DATA_HOME/zaminpanel`, default `~/.local/share/zaminpanel`.
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
/// Linux: `$XDG_CONFIG_HOME/zaminpanel`, default `~/.config/zaminpanel`.
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
        PathBuf::from(local_app_data).join("ZaminPanel")
    }
}

#[cfg(unix)]
fn data_dir_unix(xdg_data_home: Option<&str>, home: Option<PathBuf>) -> PathBuf {
    match xdg_data_home {
        Some(dir) => PathBuf::from(dir).join("zaminpanel"),
        None => match home {
            Some(home) => home.join(".local/share/zaminpanel"),
            None => PathBuf::from(".local/share/zaminpanel"),
        },
    }
}

#[cfg(unix)]
fn config_dir_unix(xdg_config_home: Option<&str>, home: Option<PathBuf>) -> PathBuf {
    match xdg_config_home {
        Some(dir) => PathBuf::from(dir).join("zaminpanel"),
        None => match home {
            Some(home) => home.join(".config/zaminpanel"),
            None => PathBuf::from(".config/zaminpanel"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn unix_dirs_follow_xdg() {
        let data = data_dir_unix(Some("/xdg/data"), Some("/home/u".into()));
        assert_eq!(data, PathBuf::from("/xdg/data/zaminpanel"));

        let config = config_dir_unix(Some("/xdg/conf"), Some("/home/u".into()));
        assert_eq!(config, PathBuf::from("/xdg/conf/zaminpanel"));

        let data_default = data_dir_unix(None, Some("/home/u".into()));
        assert_eq!(
            data_default,
            PathBuf::from("/home/u/.local/share/zaminpanel")
        );

        let config_default = config_dir_unix(None, None);
        assert_eq!(config_default, PathBuf::from(".config/zaminpanel"));
    }

    #[test]
    #[cfg(windows)]
    fn windows_dir_uses_local_app_data() {
        let dir = data_dir_windows(r"C:\Users\u\AppData\Local");
        assert_eq!(dir, PathBuf::from(r"C:\Users\u\AppData\Local\ZaminPanel"));
        let fallback = data_dir_windows("");
        assert!(!fallback.as_os_str().is_empty());
    }
}
