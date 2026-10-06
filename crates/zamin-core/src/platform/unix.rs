//! Unix process operations: `setsid`-style spawn (own process group via
//! `setpgid(0, 0)`), `/proc` start-time + boot-id identity, SIGTERM/SIGKILL
//! to the group.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::error::PlatformError;
use crate::platform::{ProcessIdentity, ProcessOps, SpawnHandle, SpawnSpec, Spawned};

struct UnixHandle {
    pid: u32,
    child: tokio::process::Child,
}

impl SpawnHandle for UnixHandle {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn child(&mut self) -> &mut tokio::process::Child {
        &mut self.child
    }

    fn force_kill_tree(&mut self) -> Result<(), PlatformError> {
        let group = -(self.pid as i32);
        let ok = unsafe { libc::kill(group, libc::SIGKILL) };
        if ok == 0 {
            return Ok(());
        }
        // Group is gone or was never created; fall back to the child itself.
        self.child
            .start_kill()
            .map_err(|_| PlatformError::ProcessGone { pid: self.pid })
    }
}

pub(crate) struct UnixProcessOps;

pub(crate) fn process() -> &'static dyn ProcessOps {
    &UnixProcessOps
}

pub fn java_exe_name() -> &'static str {
    "java"
}

/// Standard JVM install roots plus SDKMAN/asdf managed installs.
pub fn java_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for base in ["/usr/lib/jvm", "/opt/java", "/opt/jdk"] {
        if let Ok(read) = std::fs::read_dir(base) {
            for entry in read.flatten() {
                roots.push(entry.path());
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        for managed in [
            format!("{home}/.sdkman/candidates/java"),
            format!("{home}/.asdf/installs/java"),
        ] {
            if let Ok(read) = std::fs::read_dir(&managed) {
                for entry in read.flatten() {
                    roots.push(entry.path());
                }
            }
        }
    }
    roots
}

impl ProcessOps for UnixProcessOps {
    fn spawn(&self, spec: &SpawnSpec) -> Result<Spawned, PlatformError> {
        let mut command = tokio::process::Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false)
            .process_group(0); // own process group; pgid == pid

        let child = command.spawn()?;
        let pid = child.id().ok_or(PlatformError::ProcessGone { pid: 0 })?;
        Ok(Spawned::new(Box::new(UnixHandle { pid, child })))
    }

    fn identity(&self, pid: u32) -> Option<ProcessIdentity> {
        let start_marker = proc_start_marker(pid)?;
        Some(ProcessIdentity { pid, start_marker })
    }

    fn is_alive(&self, identity: &ProcessIdentity) -> bool {
        match proc_start_marker(identity.pid) {
            Some(marker) => marker == identity.start_marker,
            None => false,
        }
    }

    fn pid_exists(&self, pid: u32) -> bool {
        proc_start_marker(pid).is_some()
    }

    fn signal_graceful(&self, pid: u32) -> Result<(), PlatformError> {
        let group = -(pid as i32);
        let ok = unsafe { libc::kill(group, libc::SIGTERM) };
        if ok != 0 {
            let err = std::io::Error::last_os_error();
            return match err.raw_os_error() {
                Some(libc::ESRCH) => Err(PlatformError::ProcessGone { pid }),
                _ => Err(err.into()),
            };
        }
        Ok(())
    }

    fn force_kill(&self, pid: u32) -> Result<(), PlatformError> {
        // Processes we spawn lead their own group (pgid == pid), so the
        // group kill also covers adopted children that inherited that
        // shape. Identity must be verified by the caller (ADR-0005).
        let group = -(pid as i32);
        if unsafe { libc::kill(group, libc::SIGKILL) } == 0 {
            return Ok(());
        }
        // Group gone or unreachable: target the single process.
        let ok = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        if ok == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::ESRCH) => Err(PlatformError::ProcessGone { pid }),
            _ => Err(err.into()),
        }
    }

    fn fs_free_bytes(&self, path: &Path) -> Result<u64, PlatformError> {
        let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| {
            PlatformError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path contains a NUL byte",
            ))
        })?;
        let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
        let ok = unsafe { libc::statvfs(c_path.as_ptr(), &mut vfs) };
        if ok != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // f_bavail: free blocks for unprivileged users — the honest number
        // for a daemon that must not count reserved space it cannot use.
        Ok(vfs.f_bavail as u64 * vfs.f_frsize as u64)
    }
}

/// `/proc/<pid>/stat` starttime (field 22) plus the boot id: stable across
/// PID reuse because a new boot resets starttime *and* the pid namespace is
/// per-boot.
fn proc_start_marker(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();

    // comm can contain spaces and parentheses; fields resume after the
    // final ')'. Field 22 (starttime) is offset 19 from the state field.
    let after_comm = stat.rsplit_once(')').map(|(_, rest)| rest)?;
    let starttime = after_comm.split_whitespace().nth(19)?;

    Some(format!("{boot_id}/{starttime}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_of_self_is_stable() {
        let me = std::process::id();
        let identity = proc_start_marker(me).expect("own /proc entry exists");
        assert_eq!(proc_start_marker(me), Some(identity.clone()));
        assert!(identity.contains('/'));
    }

    #[test]
    fn identity_of_missing_pid_is_none() {
        // pid 4 is the kernel on Linux; a giant pid does not exist.
        assert!(proc_start_marker(u32::MAX / 2).is_none());
    }

    #[test]
    fn working_dir_type_check() {
        // Compile-time shape check for SpawnSpec on this platform.
        let spec = SpawnSpec {
            program: PathBuf::from("true"),
            args: vec![],
            working_dir: PathBuf::from("/tmp"),
        };
        assert_eq!(spec.working_dir, PathBuf::from("/tmp"));
    }

    #[test]
    fn fs_free_bytes_reports_a_plausible_tmpfs() {
        let free = process()
            .fs_free_bytes(Path::new("/tmp"))
            .expect("statvfs on /tmp works");
        assert!(free > 0, "free space must be positive, got {free}");
    }
}
