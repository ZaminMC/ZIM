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

#[cfg(unix)]
pub mod linux {
    use std::path::{Path, PathBuf};

    const APP_DESKTOP: &str = "mc.zamin.zim.desktop";

    /// The autostart entry path for a config home (`$XDG_CONFIG_HOME`).
    pub fn autostart_file(config_home: &Path) -> PathBuf {
        config_home.join("autostart").join(APP_DESKTOP)
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
             Exec={exec}\n\
             Icon=mc.zamin.zim\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n"
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
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VALUE_NAME: &str = "ZIM";

    fn open_run_key() -> Result<RegKey, String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN_KEY)
            .map_err(|error| format!("cannot open the Run key: {error}"))
    }

    pub fn get() -> Result<Option<bool>, String> {
        let key = open_run_key()?;
        Ok(Some(key.get_value::<String, _>(RUN_VALUE_NAME).is_ok()))
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if !enabled {
            let key = hkcu
                .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
                .map_err(|error| format!("cannot open the Run key: {error}"))?;
            key.delete_value(RUN_VALUE_NAME)
                .or_else(|error| match error.kind() {
                    std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(error),
                })
                .map_err(|error| format!("could not delete the Run value: {error}"))?;
            return Ok(());
        }
        let exe = std::env::current_exe()
            .map_err(|error| format!("cannot resolve the panel path: {error}"))?;
        let command = format!("\"{}\"", exe.display());
        let key = open_run_key()?;
        key.set_value(RUN_VALUE_NAME, &command)
            .map_err(|error| format!("could not write the Run value: {error}"))
    }
}

// --- platform dispatch ---------------------------------------------------------

#[cfg(unix)]
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
    #[cfg(unix)]
    return linux::get(&config_home()?);
    // The Windows half answers Result<Option<bool>>: the outer Result is
    // the registry probe ("cannot be determined"), the inner Option is the
    // Run key's absence ("autostart is off") — flatten, so a failed probe
    // reads as "unavailable" and an absent key reads as "off".
    #[cfg(windows)]
    return windows::get().ok().flatten();
}

pub fn set(enabled: bool) -> Result<(), String> {
    #[cfg(unix)]
    return linux::set(
        &config_home().ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".to_owned())?,
        enabled,
    );
    #[cfg(windows)]
    return windows::set(enabled);
}

#[cfg(all(test, unix))]
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
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::windows;

    #[test]
    fn run_key_roundtrip() {
        windows::set(false).expect("clearing a value that may not exist");
        assert_eq!(windows::get().expect("readable"), Some(false));
        windows::set(true).expect("autostart on");
        assert_eq!(windows::get().expect("readable"), Some(true));
        windows::set(false).expect("autostart off");
        assert_eq!(windows::get().expect("readable"), Some(false));
    }
}
