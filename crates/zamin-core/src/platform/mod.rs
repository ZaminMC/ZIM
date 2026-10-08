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
pub mod private_file;

// Platform-specific discovery data (install roots, executable naming).
pub use imp::{java_exe_name, java_install_roots};

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::PlatformError;

/// One raw process sample from the OS. `cpu_time` is the cumulative
/// user+kernel CPU time; CPU *percent* is computed by the caller from two
/// samples — the OS exposes counters, not rates. `rss_bytes` is the
/// resident set where the OS exposes it unprivileged; `None` means "not
/// measured" (honest absence, never a fake number — the metrics rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessSample {
    pub cpu_time: Duration,
    pub rss_bytes: Option<u64>,
}

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

    /// Whether ANY live process owns `pid` right now — marker-agnostic.
    /// Adoption uses this to tell "the recorded server is gone" (safe to
    /// reset) from "the pid was reused by something else" (never touch).
    fn pid_exists(&self, pid: u32) -> bool;

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

    /// Raw resource counters for a live process, or `None` if it does not
    /// exist (or the OS refuses the unprivileged read). One cheap read per
    /// call — the metrics sampler runs this at 1 Hz per running server and
    /// the sampler budget is < 1% of a core (PERFORMANCE-BUDGETS).
    fn sample_process(&self, pid: u32) -> Option<ProcessSample>;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_self_reports_counters() {
        // Clock ticks are 10 ms granular; burn CPU so the counter is
        // guaranteed nonzero at that granularity.
        let start = std::time::Instant::now();
        let mut sink = 0u64;
        while start.elapsed() < Duration::from_millis(60) {
            sink = sink.wrapping_add(sink | 0x9e37_79b9_7f4a_7c15);
        }
        std::hint::black_box(sink);
        // This test process exists and has consumed some CPU by running.
        let sample = process().sample_process(std::process::id());
        let sample = sample.expect("own process is sampleable");
        assert!(sample.cpu_time > Duration::ZERO, "test already burned CPU");
        // RSS is measured on every platform we ship; None would mean the
        // platform read broke silently.
        let rss = sample.rss_bytes.expect("rss is measurable");
        assert!(rss > 1_000_000, "a rust test binary uses > 1 MB, got {rss}");
    }

    #[test]
    fn sampling_missing_pid_is_none() {
        assert!(process().sample_process(u32::MAX / 2).is_none());
    }

    #[test]
    fn sampling_is_cheap() {
        // The 1 Hz budget needs each read far below 10 ms; a few file reads
        // or one syscall pair land in the microseconds.
        let start = std::time::Instant::now();
        for _ in 0..100 {
            let _ = process().sample_process(std::process::id());
        }
        let per_call = start.elapsed() / 100;
        assert!(
            per_call < Duration::from_millis(10),
            "one sample must be cheap, measured {per_call:?}"
        );
    }
}
