//! Login autostart (Phase 7 deliverable; ARCH-REVIEW §12.1 lists the seam:
//! Task Scheduler / Run key on Windows, the freedesktop equivalent on
//! Linux). This host crate sits outside the workspace platform seam —
//! ADR-0008 guards the workspace crates, and the thin Tauri shell is the
//! one place OS-conditional integration code is allowed to live beside the
//! webview glue.
//!
//! Choice of mechanisms, deliberately boring:
//! - Linux: an XDG autostart desktop entry in `$XDG_CONFIG_HOME/autostart`
//!   — no root, honored by GNOME and KDE, trivially removable.
//! - Windows: the per-user `HKCU\...\CurrentVersion\Run` key — no admin,
//!   visible in Task Manager's startup list.
//!
//! The systemd user unit / Task Scheduler task variants belong to Phase 8
//! (services), where the daemon gets a service story of its own.
//!
//! Pure helpers take their environment as parameters; the stateful glue is
//! thin. Tests never touch the machine's real autostart state except the
//! Windows HKCU round-trip, which only runs on the Windows CI lane.

// --- Linux: XDG autostart ----------------------------------------------------

// EXPLICITLY Linux: the XDG autostart spec is the freedesktop mechanism
// GNOME and KDE honor, and `target_os = "linux"` is what builds it. The
// `cfg(unix)` this used to wear also compiled the module on macOS — where
// there is no XDG autostart and the entry would simply be ignored. Other
// unixes get the honest "unavailable" from the dispatch below until a
// platform answer (LaunchAgents on macOS) is actually wired.
#[cfg(target_os = "linux")]
pub mod linux {
    use std::path::{Path, PathBuf};

    const APP_DESKTOP: &str = "mc.zamin.zim.desktop";

    /// The autostart entry path for a config home (`$XDG_CONFIG_HOME`).
    pub fn autostart_file(config_home: &Path) -> PathBuf {
        config_home.join("autostart").join(APP_DESKTOP)
    }

    /// The Exec value for a command path, quoted per the Desktop Entry
    /// Spec: the argument is wrapped in double quotes, and the characters
    /// that are reserved inside double quotes (backslash, double quote,
    /// backtick, dollar sign) are backslash-escaped. A path like
    /// "/home/zamin/My Apps/ZIM/zim" used to land raw — one space in the
    /// install path and the entry parsed as two arguments, or worse.
    pub fn exec_field(exec: &str) -> String {
        let mut quoted = String::with_capacity(exec.len() + 2);
        quoted.push('"');
        for c in exec.chars() {
            if matches!(c, '\\' | '"' | '`' | '$') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted
    }

    /// The entry content. `exec` is the panel launch path — inside an
    /// AppImage that must be `$APPIMAGE` (the mount point under /tmp dies
    /// with the session; the squashfs source does not).
    pub fn entry_content(exec: &str) -> String {
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=ZIM\n\
             Comment=ZIM starts with your session so the daemon is ready\n\
             Exec={}\n\
             Icon=mc.zamin.zim\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n",
            exec_field(exec)
        )
    }

    /// What the autostart entry should say about launching this panel.
    pub fn desired_exec() -> Result<String, String> {
        if let Ok(appimage) = std::env::var("APPIMAGE") {
            if !appimage.is_empty() {
                return Ok(appimage);
            }
        }
        std::env::current_exe()
            .map(|exe| exe.to_string_lossy().into_owned())
            .map_err(|error| format!("cannot resolve the panel path: {error}"))
    }

    pub fn get(config_home: &Path) -> Option<bool> {
        Some(autostart_file(config_home).is_file())
    }

    pub fn set(config_home: &Path, enabled: bool) -> Result<(), String> {
        let file = autostart_file(config_home);
        if !enabled {
            return match std::fs::remove_file(&file) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!("could not remove the autostart entry: {error}")),
            };
        }
        let exec = desired_exec()?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create the autostart directory: {error}"))?;
        }
        std::fs::write(&file, entry_content(&exec))
            .map_err(|error| format!("could not write the autostart entry: {error}"))
    }
}

// --- Windows: HKCU Run key -----------------------------------------------------

#[cfg(windows)]
pub mod windows {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VALUE_NAME: &str = "ZIM";
    /// The roundtrip's own hive location: the REAL Run key is
    /// autorun-protected and CI policy denies the write to a cargo test
    /// binary (Defender ASR, "Access is denied"), so the mechanics prove
    /// themselves on a quiet subkey while the real path answers
    /// read-only. The production path stays the literal Run key.
    #[cfg(test)]
    pub(crate) const TEST_RUN_KEY: &str = r"Software\ZIM\Tests\AutostartRun";

    pub fn get() -> Result<Option<bool>, String> {
        get_at(RUN_KEY)
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        set_at(RUN_KEY, enabled)
    }

    pub(super) fn get_at(path: &str) -> Result<Option<bool>, String> {
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(path, KEY_QUERY_VALUE)
            .map_err(|error| format!("cannot open the key: {error}"))?;
        Ok(Some(key.get_value::<String, _>(RUN_VALUE_NAME).is_ok()))
    }

    pub(super) fn set_at(path: &str, enabled: bool) -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if !enabled {
            let key = hkcu
                .open_subkey_with_flags(path, KEY_SET_VALUE)
                .map_err(|error| format!("cannot open the key: {error}"))?;
            key.delete_value(RUN_VALUE_NAME)
                .or_else(|error| match error.kind() {
                    std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(error),
                })
                .map_err(|error| format!("could not delete the value: {error}"))?;
            return Ok(());
        }
        let exe = std::env::current_exe()
            .map_err(|error| format!("cannot resolve the panel path: {error}"))?;
        let command = format!("\"{}\"", exe.display());
        // The write needs KEY_SET_VALUE — the read-only open the old
        // enable path used was refused everywhere (the toggle could
        // never turn ON; the roundtrip test's first real run caught it).
        let key = hkcu
            .open_subkey_with_flags(path, KEY_SET_VALUE)
            .map_err(|error| format!("cannot open the key: {error}"))?;
        key.set_value(RUN_VALUE_NAME, &command)
            .map_err(|error| format!("could not write the value: {error}"))
    }

    // The tests' subkey does not exist until a test creates it — and
    // RegCreateKeyEx never makes intermediates, so the chain builds one
    // component at a time.
    #[cfg(test)]
    pub(super) fn ensure_test_key() -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let mut walked = String::new();
        for component in TEST_RUN_KEY.split('\\') {
            if !walked.is_empty() {
                walked.push('\\');
            }
            walked.push_str(component);
            hkcu.create_subkey(&walked)
                .map(|_| ())
                .map_err(|error| format!("could not create the test key: {error}"))?;
        }
        Ok(())
    }
}

// --- platform dispatch ---------------------------------------------------------

// The XDG config resolution belongs to the Linux mechanism only — a
// macOS build has no use for XDG_CONFIG_HOME's autostart subdirectory.
#[cfg(target_os = "linux")]
fn config_home() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".config"))
        })
}

/// `Some(enabled)` when the platform's autostart state is knowable; the
/// webview renders `None` as "unavailable".
pub fn get() -> Option<bool> {
    #[cfg(target_os = "linux")]
    return linux::get(&config_home()?);
    // The Windows half answers Result<Option<bool>>: the outer Result is
    // the registry probe ("cannot be determined"), the inner Option is the
    // Run key's absence ("autostart is off") — flatten, so a failed probe
    // reads as "unavailable" and an absent key reads as "off".
    #[cfg(windows)]
    return windows::get().ok().flatten();
    // macOS and every other unix: no mechanism is wired (see the module
    // gate above) — the honest answer is "unavailable", never Linux's.
    #[cfg(not(any(target_os = "linux", windows)))]
    return None;
}

pub fn set(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return linux::set(
        &config_home().ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".to_owned())?,
        enabled,
    );
    #[cfg(windows)]
    return windows::set(enabled);
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = enabled;
        Err("autostart is not supported on this platform yet".to_owned())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_home(tag: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "zamin-autostart-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir builds");
        dir
    }

    #[test]
    fn get_reflects_set_on_and_off() {
        let home = temp_config_home("roundtrip");
        assert_eq!(linux::get(&home), Some(false), "absent entry = off");
        linux::set(&home, true).expect("autostart on");
        assert_eq!(linux::get(&home), Some(true));
        let file = linux::autostart_file(&home);
        let content = fs::read_to_string(&file).expect("entry readable");
        assert!(content.starts_with("[Desktop Entry]\n"));
        assert!(content.contains("Type=Application\n"));
        assert!(content.contains("\nExec="));
        assert!(content.contains("X-GNOME-Autostart-enabled=true\n"));
        linux::set(&home, false).expect("autostart off");
        assert_eq!(linux::get(&home), Some(false));
        assert!(!file.exists());
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn set_off_without_an_entry_is_ok() {
        let home = temp_config_home("off-quiet");
        linux::set(&home, false).expect("removing nothing is fine");
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn set_on_creates_missing_directories() {
        let home = temp_config_home("mkdir");
        linux::set(&home, true).expect("autostart on with fresh dirs");
        assert!(linux::autostart_file(&home).is_file());
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn exec_field_quotes_paths_with_spaces() {
        // The audit's own case: a valid install path with a space used to
        // produce an entry whose command line split into two arguments.
        assert_eq!(
            linux::exec_field("/home/zamin/My Apps/ZIM/zim"),
            "\"/home/zamin/My Apps/ZIM/zim\""
        );
    }

    #[test]
    fn exec_field_escapes_the_reserved_characters() {
        // Inside double quotes the spec reserves backslash, double quote,
        // backtick, and dollar sign — each must ride a backslash.
        assert_eq!(
            linux::exec_field(r"/opt/wei\rd"),
            "\"/opt/wei\\\\rd\"",
            "a backslash in the path doubles"
        );
        let escaped = linux::exec_field("/a\"b`c$d");
        assert_eq!(escaped, "\"/a\\\"b\\`c\\$d\"");
        // And the entry embeds the quoted form where Exec= sits.
        let content = linux::entry_content("/home/z/My Apps/zim");
        assert!(
            content.contains("Exec=\"/home/z/My Apps/zim\"\n"),
            "{content}"
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::windows;
    use super::windows::TEST_RUN_KEY;

    #[test]
    fn run_key_roundtrip() {
        // The mechanics on the tests' own subkey — the real Run key is
        // autorun-protected and CI policy denies the write to this
        // binary (Defender ASR); the production path's access rights are
        // the same code these lines execute.
        windows::ensure_test_key().expect("the test key creates");
        windows::set_at(TEST_RUN_KEY, false).expect("clearing a value that may not exist");
        assert_eq!(
            windows::get_at(TEST_RUN_KEY).expect("readable"),
            Some(false)
        );
        windows::set_at(TEST_RUN_KEY, true).expect("autostart on");
        assert_eq!(windows::get_at(TEST_RUN_KEY).expect("readable"), Some(true));
        windows::set_at(TEST_RUN_KEY, false).expect("autostart off");
        assert_eq!(
            windows::get_at(TEST_RUN_KEY).expect("readable"),
            Some(false)
        );
    }

    #[test]
    fn the_real_run_key_is_readable() {
        // The read-only smoke of the production path: the literal Run
        // key opens and answers — the write is exercised by the
        // roundtrip above on the same code.
        let state = windows::get();
        if state.is_err() {
            // A runner without the key at all: absence is an honest
            // answer, not a failure — but Access-denied on a READ is
            // worth naming, so the panic carries the sentence.
            panic!("the real Run key is not readable: {:?}", state);
        }
    }
}
