//! The platform seam (ADR-0008): the only place in `zamin-core` where
//! OS-conditional code and process spawning may exist.
//!
//! Everything above this module operates against the traits defined here;
//! the factory selects the implementation for the host OS at compile time.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
use windows as imp;

pub mod paths;

// Platform-specific discovery data (install roots, executable naming).
pub use imp::{java_exe_name, java_install_roots};

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::PlatformError;

/// Everything needed to spawn a server process. stdin/stdout/stderr are
/// always piped; the log pipeline owns them.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
}

/// A spawned process and its platform-specific kill handle.
pub struct Spawned {
    pid: u32,
    handle: Box<dyn SpawnHandle>,
}

impl Spawned {
    pub fn new(handle: Box<dyn SpawnHandle>) -> Self {
        Spawned {
            pid: handle.pid(),
            handle,
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn handle(&mut self) -> &mut dyn SpawnHandle {
        self.handle.as_mut()
    }
}

/// Per-spawn operations. The daemon owns the child; killing is explicit and
/// tree-wide (ADR-0005), never a side effect of a handle being dropped.
/// `Sync` so an actor holding a handle can be awaited from any worker.
pub trait SpawnHandle: Send + Sync {
    fn pid(&self) -> u32;
    /// The piped child process: stdin/stdout/stderr and exit waiting.
    fn child(&mut self) -> &mut tokio::process::Child;
    /// Ladder step 4: force-terminate the whole process tree.
    fn force_kill_tree(&mut self) -> Result<(), PlatformError>;
}

/// Verified process identity: PID plus a start marker. A PID alone is
/// unsafe on both platforms — reuse makes "PID exists" mean nothing
/// (ADR-0005, hard safety invariant).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Windows: process creation time (FILETIME). Linux: `/proc` starttime
    /// plus the boot id.
    pub start_marker: String,
}

pub trait ProcessOps: Send + Sync {
    /// Spawn with platform-appropriate flags: hidden window + new process
    /// group on Windows, `setsid` on Linux. Graceful termination later
    /// targets the group, whose id equals the child's pid.
    fn spawn(&self, spec: &SpawnSpec) -> Result<Spawned, PlatformError>;

    /// Identity of a live process, or `None` if it does not exist.
    fn identity(&self, pid: u32) -> Option<ProcessIdentity>;

    /// True only if `pid` is alive *and* its start marker matches.
    fn is_alive(&self, identity: &ProcessIdentity) -> bool;

    /// Ladder step 3: best-effort OS-graceful signal to the process group
    /// (CTRL_BREAK on Windows, SIGTERM on Linux). Windows signals only
    /// reach processes sharing the daemon's console; failure is reported,
    /// never hidden.
    fn signal_graceful(&self, pid: u32) -> Result<(), PlatformError>;

    /// Ladder step 4 for adopted servers: force-terminate by PID, covering
    /// the process group where the platform allows it. The caller MUST have
    /// verified the identity (PID + start marker) beforehand — no code path
    /// may kill a process whose identity was not verified (ADR-0005). For
    /// adopted Windows servers the original Job Object is unreachable, so
    /// the kill degrades to the single verified process.
    fn force_kill(&self, pid: u32) -> Result<(), PlatformError>;

    /// Bytes still free on the filesystem containing `path` (unprivileged
    /// view). Used by the disk-headroom preflight check (ADR-0005).
    fn fs_free_bytes(&self, path: &Path) -> Result<u64, PlatformError>;

    /// Run a short-lived process to completion and capture its output.
    /// Blocking by design; async callers go through `spawn_blocking`.
    /// Used by, e.g., Java runtime inspection.
    fn run_capture(&self, spec: &SpawnSpec, timeout: Duration) -> Result<RunOutput, PlatformError> {
        use std::io::Read;
        use std::process::Stdio;

        let mut command = std::process::Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW_SPAWN);
        }

        let mut child = command.spawn()?;
        let deadline = std::time::Instant::now() + timeout;
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();

        let stdout_reader = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(pipe) = stdout_pipe.as_mut() {
                let _ = pipe.read_to_string(&mut buf);
            }
            buf
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(pipe) = stderr_pipe.as_mut() {
                let _ = pipe.read_to_string(&mut buf);
            }
            buf
        });

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let stdout = stdout_reader.join().unwrap_or_default();
                    let stderr = stderr_reader.join().unwrap_or_default();
                    return Ok(RunOutput {
                        exit_code: status.code().unwrap_or(-1),
                        stdout,
                        stderr,
                    });
                }
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(PlatformError::RunTimedOut {
                            program: spec.program.clone(),
                            timeout,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(source) => return Err(PlatformError::Io(source)),
            }
        }
    }
}

/// Output of a completed short-lived process.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Hidden window for short-lived helper processes (`run_capture`); the
/// server spawn adds the process-group flag separately.
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW_SPAWN: u32 = 0x0800_0000;

pub fn process() -> &'static dyn ProcessOps {
    imp::process()
}
