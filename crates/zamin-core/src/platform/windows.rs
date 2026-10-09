//! Windows process operations: hidden-window spawn into a new process
//! group and a Job Object (no kill-on-close — the daemon dying must never
//! kill servers, ADR-0001), creation-time process identity, CTRL_BREAK
//! graceful signal, job-based tree termination — and, when the spawn asks
//! for it, the AppContainer sandbox (`windows_sandbox`): the OS-enforced
//! filesystem + network boundary the daemon's path checks can never be.

use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectCpuRateControlInformation,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
    JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
};
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::SystemInformation::GetSystemInfo;
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, TerminateProcess, IO_COUNTERS, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};

use crate::error::PlatformError;
use crate::platform::{
    JobNotice, ProcessIdentity, ProcessOps, ProcessSample, SpawnHandle, SpawnLimits, SpawnSpec,
    Spawned,
};

#[path = "windows_sandbox.rs"]
pub(crate) mod sandbox;
pub use sandbox::{cleanup_container, container_profile_name};

/// Create a job with the spec's limits configured (before any process is
/// assigned — the enforcement boundary exists before its first inmate).
unsafe fn create_job_with_limits(limits: &SpawnLimits) -> Result<HANDLE, PlatformError> {
    let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
    if job.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    if let Err(error) = configure_job_limits(job, limits) {
        CloseHandle(job);
        return Err(error);
    }
    Ok(job)
}

/// The pid behind a freshly created raw process handle.
fn process_id_of(process: HANDLE) -> Option<u32> {
    let pid = unsafe { windows_sys::Win32::System::Threading::GetProcessId(process) };
    (pid != 0).then_some(pid)
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0200;

/// Job limit flag bits (winbase.h / JOB_OBJECT_LIMIT_*).
const JOB_OBJECT_LIMIT_ACTIVE_PROCESS: u32 = 0x0000_0008;
const JOB_OBJECT_LIMIT_PROCESS_MEMORY: u32 = 0x0000_0100;
const JOB_OBJECT_LIMIT_JOB_MEMORY: u32 = 0x0000_0200;
/// CPU rate control flags (winnt.h / JOB_OBJECT_CPU_RATE_CONTROL_*).
const JOB_OBJECT_CPU_RATE_CONTROL_ENABLE: u32 = 0x0000_0001;
/// windows-sys 0.59 exports no HARD_ENABLE bit (its HARD_CAP=4 is the
/// weight-based flag's value in current winnt.h), so the hard-enable
/// bit carries its winnt.h value with the citation here.
const JOB_OBJECT_CPU_RATE_CONTROL_HARD_ENABLE: u32 = 0x0000_0002;

/// Logical processors on this host (the CPU-rate scale's denominator).
fn logical_processors() -> u32 {
    let mut info = unsafe { std::mem::zeroed() };
    unsafe { GetSystemInfo(&mut info) };
    info.dwNumberOfProcessors.max(1)
}

/// Translate the config's percent-of-one-core ceiling into Windows' own
/// whole-machine CPU-rate scale (1..=10000 where 10000 = the WHOLE
/// machine): wanted = P% of one core out of N cores = (P/100)/N of the
/// machine = 100·P/N in CpuRate units.
fn cpu_rate_from_percent(percent_of_core: u32) -> u32 {
    let cores = logical_processors();
    let rate = (percent_of_core as u64 * 100) / cores as u64;
    rate.clamp(1, 10_000) as u32
}

/// The enforcement step: write the spec's limits into a FRESH job object
/// BEFORE any process is assigned, so the server's first instruction
/// already runs bounded. Every limit set here is OS-enforced: a plugin
/// that allocates past the memory cap gets its allocation refused; a
/// tree that wants more CPU than its rate gets throttled by the
/// scheduler; the active-process cap bounds child-process bombs.
fn configure_job_limits(job: HANDLE, limits: &SpawnLimits) -> Result<(), PlatformError> {
    // Memory + process count ride the extended limit structure.
    let mut extended = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
            PerProcessUserTimeLimit: 0,
            PerJobUserTimeLimit: 0,
            LimitFlags: 0,
            MinimumWorkingSetSize: 0,
            MaximumWorkingSetSize: 0,
            ActiveProcessLimit: 0,
            Affinity: 0,
            PriorityClass: 0,
            SchedulingClass: 0,
        },
        IoInfo: IO_COUNTERS {
            ReadOperationCount: 0,
            WriteOperationCount: 0,
            OtherOperationCount: 0,
            ReadTransferCount: 0,
            WriteTransferCount: 0,
            OtherTransferCount: 0,
        },
        ProcessMemoryLimit: 0,
        JobMemoryLimit: 0,
        PeakProcessMemoryUsed: 0,
        PeakJobMemoryUsed: 0,
    };
    if let Some(memory) = limits.memory_bytes {
        extended.BasicLimitInformation.LimitFlags |=
            JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_JOB_MEMORY;
        // The limit fields are usize (pointer-width) in windows-sys; a
        // byte budget fits both 64-bit and 32-bit windows targets
        // scaled down, and this build targets 64-bit.
        extended.ProcessMemoryLimit = memory as usize;
        // The job-wide cap matches the per-process cap: the tree's total
        // commit may not exceed what one process may, so N children
        // cannot multiply their way past the budget.
        extended.JobMemoryLimit = memory as usize;
    }
    if let Some(count) = limits.process_count {
        extended.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        extended.BasicLimitInformation.ActiveProcessLimit = count;
    }
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &extended as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    // CPU rate control lives in its own information class. The config
    // speaks percent-of-one-core ("400 = four cores"); Windows speaks
    // whole-machine rate (10000 = the entire machine). The conversion
    // uses the host's logical-processor count so the operator's number
    // means the same thing on a 4-core laptop and a 64-core server.
    if let Some(percent) = limits.cpu_percent {
        let cpu_rate = cpu_rate_from_percent(percent);
        let control = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
            ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                | JOB_OBJECT_CPU_RATE_CONTROL_HARD_ENABLE,
            // The rate rides the struct's union (CpuRate | Weight).
            Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 { CpuRate: cpu_rate },
        };
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectCpuRateControlInformation,
                &control as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

/// Owns the job handle; closing it on drop is safe because kill-on-close is
/// NOT set — a dropped job never terminates processes. A HANDLE is an
/// integer token, safe to move between threads.
struct JobHandle(HANDLE);

unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// The two ways this module owns children: the plain tokio spawn
/// (unsandboxed helper/server starts) and the raw sandboxed spawn (the
/// AppContainer path, whose pipes/handles the trait converts lazily).
/// The tokio child rides a Box — the raw variant's five handles would
/// otherwise pad every Tokio variant with ~200 dead bytes.
enum ChildFlavor {
    Tokio(Box<tokio::process::Child>),
    Raw {
        stdin: Option<std::os::windows::io::OwnedHandle>,
        stdout: Option<std::os::windows::io::OwnedHandle>,
        stderr: Option<std::os::windows::io::OwnedHandle>,
        process: std::os::windows::io::OwnedHandle,
        thread: std::os::windows::io::OwnedHandle,
    },
}

struct WindowsHandle {
    pid: u32,
    flavor: ChildFlavor,
    job: Option<JobHandle>,
    /// The job object's completion-port notices; the watcher thread ends
    /// when this receiver (and its sender) drop. The Mutex exists for
    /// the trait's `Sync` promise — the actor is the only taker.
    notices: Option<std::sync::Mutex<std::sync::mpsc::Receiver<JobNotice>>>,
}

impl SpawnHandle for WindowsHandle {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn take_stdin(
        &mut self,
    ) -> Result<Option<Box<dyn tokio::io::AsyncWrite + Send + Sync + Unpin>>, PlatformError> {
        match &mut self.flavor {
            ChildFlavor::Tokio(child) => Ok(child
                .stdin
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncWrite + Send + Sync + Unpin>)),
            ChildFlavor::Raw { stdin, .. } => Ok(stdin.take().map(sandbox::bridge_writer)),
        }
    }

    fn take_stdout(
        &mut self,
    ) -> Result<Option<Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>>, PlatformError> {
        match &mut self.flavor {
            ChildFlavor::Tokio(child) => Ok(child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>)),
            ChildFlavor::Raw { stdout, .. } => Ok(stdout.take().map(sandbox::bridge_reader)),
        }
    }

    fn take_stderr(
        &mut self,
    ) -> Result<Option<Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>>, PlatformError> {
        match &mut self.flavor {
            ChildFlavor::Tokio(child) => Ok(child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>)),
            ChildFlavor::Raw { stderr, .. } => Ok(stderr.take().map(sandbox::bridge_reader)),
        }
    }

    fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>, PlatformError> {
        match &mut self.flavor {
            ChildFlavor::Tokio(child) => Ok(child.try_wait()?),
            ChildFlavor::Raw { process, .. } => sandbox::try_wait_raw(process),
        }
    }

    fn force_kill_tree(&mut self) -> Result<(), PlatformError> {
        if let Some(job) = &self.job {
            if unsafe { TerminateJobObject(job.0, 1) } != 0 {
                return Ok(());
            }
        }
        match &mut self.flavor {
            ChildFlavor::Tokio(child) => child
                .start_kill()
                .map_err(|_| PlatformError::ProcessGone { pid: self.pid }),
            ChildFlavor::Raw { process, .. } => sandbox::kill_raw(process),
        }
    }

    fn drain_notices(&mut self) -> Vec<JobNotice> {
        let Some(rx) = &self.notices else {
            return Vec::new();
        };
        let Ok(rx) = rx.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(notice) = rx.try_recv() {
            out.push(notice);
        }
        out
    }
}

pub(crate) struct WindowsProcessOps;

pub(crate) fn process() -> &'static dyn ProcessOps {
    &WindowsProcessOps
}

pub fn java_exe_name() -> &'static str {
    "java.exe"
}

/// Vendor directories under Program Files that ship JDKs.
pub fn java_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let bases = [
        std::env::var("ProgramFiles").unwrap_or_default(),
        std::env::var("ProgramFiles(x86)").unwrap_or_default(),
    ];
    let vendors = [
        "Java",
        "Eclipse Adoptium",
        "Zulu",
        "Microsoft",
        "Amazon Corretto",
        "AdoptOpenJDK",
    ];
    for base in bases {
        if base.is_empty() {
            continue;
        }
        for vendor in vendors {
            let vendor_dir = PathBuf::from(&base).join(vendor);
            if let Ok(read) = std::fs::read_dir(&vendor_dir) {
                for entry in read.flatten() {
                    roots.push(entry.path());
                }
            }
        }
    }
    roots
}

fn filetime_u64(ft: &FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

impl ProcessOps for WindowsProcessOps {
    fn spawn(&self, spec: &SpawnSpec) -> Result<Spawned, PlatformError> {
        // Two paths share the tail (job creation + limits + assignment +
        // watcher):
        // - plain: tokio's Command (the pre-sandbox behavior, unchanged);
        // - sandboxed: the raw AppContainer spawn. THE CONTRACT: a spawn
        //   that asked for the boundary either lands inside it or fails
        //   — never a silent downgrade to unsandboxed.
        let (pid, mut flavor) = match &spec.sandbox {
            None => {
                let mut command = tokio::process::Command::new(&spec.program);
                command
                    .args(&spec.args)
                    .current_dir(&spec.working_dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .kill_on_drop(false)
                    .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
                let child = command.spawn()?;
                let pid = child.id().ok_or(PlatformError::ProcessGone { pid: 0 })?;
                (pid, ChildFlavor::Tokio(Box::new(child)))
            }
            Some(sandbox_spawn) => {
                let container = sandbox::ensure_container(&sandbox_spawn.container_name)?;
                let raw = sandbox::spawn_appcontainer(spec, &container, sandbox_spawn.network)?;
                let pid = process_id_of(raw.process.as_raw_handle())
                    .ok_or(PlatformError::ProcessGone { pid: 0 })?;
                (
                    pid,
                    ChildFlavor::Raw {
                        stdin: Some(raw.stdin),
                        stdout: Some(raw.stdout),
                        stderr: Some(raw.stderr),
                        process: raw.process,
                        thread: raw.thread,
                    },
                )
            }
        };

        // The job object is the tree-kill mechanism AND the enforcement
        // boundary. Without kill-on-close, the job outlives the daemon
        // and adoption (ADR-0005) still sees a live process. Limits are
        // configured BEFORE the process is assigned; a limit failure
        // fails the spawn rather than silently running unbounded — on
        // BOTH paths now: the old plain path silently skipped a failed
        // job (an unbounded server), which the enforcement rule forbids.
        // The sandboxed child is still suspended here, so nothing has
        // executed before the assignment.
        let job: JobHandle = match &mut flavor {
            ChildFlavor::Tokio(child) => {
                let Some(process_handle) = child.raw_handle() else {
                    return Err(PlatformError::ProcessGone { pid });
                };
                let job = unsafe { create_job_with_limits(&spec.limits)? };
                if unsafe { AssignProcessToJobObject(job, process_handle) } == 0 {
                    unsafe { CloseHandle(job) };
                    return Err(PlatformError::Io(std::io::Error::last_os_error()));
                }
                JobHandle(job)
            }
            ChildFlavor::Raw { process, .. } => {
                let job = unsafe { create_job_with_limits(&spec.limits)? };
                if unsafe { AssignProcessToJobObject(job, process.as_raw_handle()) } == 0 {
                    unsafe { CloseHandle(job) };
                    return Err(PlatformError::Io(std::io::Error::last_os_error()));
                }
                JobHandle(job)
            }
        };

        // The sandboxed child's main thread is still suspended: with the
        // boundary assigned, let it run. A resume failure terminates the
        // process and fails the spawn — no zombie, no unbounded start.
        if let ChildFlavor::Raw {
            process, thread, ..
        } = &flavor
        {
            sandbox::resume(process, thread)?;
        }

        // The completion-port watcher: OS-refused allocations and forks
        // become JobNotices the actor turns into security events.
        let notices = {
            let (tx, rx) = std::sync::mpsc::channel();
            match sandbox::spawn_job_watcher(job.0, tx) {
                Ok(()) => Some(std::sync::Mutex::new(rx)),
                // A watcher that cannot start is logged-then-lived-with:
                // the limits themselves remain enforced; only the OS's
                // refusal REPORTING is lost, and the honest error says so.
                Err(error) => {
                    eprintln!("job notification watcher unavailable: {error}");
                    None
                }
            }
        };

        Ok(Spawned::new(Box::new(WindowsHandle {
            pid,
            flavor,
            job: Some(job),
            notices,
        })))
    }

    fn identity(&self, pid: u32) -> Option<ProcessIdentity> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut creation = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut exit = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut kernel = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut user = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let ok = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);
            CloseHandle(process);
            if ok == 0 {
                return None;
            }
            Some(ProcessIdentity {
                pid,
                start_marker: filetime_u64(&creation).to_string(),
            })
        }
    }

    fn is_alive(&self, identity: &ProcessIdentity) -> bool {
        self.identity(identity.pid)
            .is_some_and(|found| found.start_marker == identity.start_marker)
    }

    fn pid_exists(&self, pid: u32) -> bool {
        self.identity(pid).is_some()
    }

    fn signal_graceful(&self, pid: u32) -> Result<(), PlatformError> {
        // Works only while the child shares the daemon's console; a
        // windowless daemon still has a hidden console that children
        // inherit, so the common case succeeds. Failure is reported and the
        // ladder falls through to TerminateProcess (ADR-0005).
        let ok = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) };
        if ok == 0 {
            return Err(PlatformError::GracefulSignalUnsupported);
        }
        Ok(())
    }

    fn force_kill(&self, pid: u32) -> Result<(), PlatformError> {
        // Adopted servers were spawned by a previous daemon, so the Job
        // Object that provides tree-kill is unreachable here; the kill
        // degrades to the single verified process. Identity must be
        // verified by the caller (ADR-0005).
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                return Err(PlatformError::ProcessGone { pid });
            }
            let ok = TerminateProcess(process, 1);
            CloseHandle(process);
            if ok == 0 {
                return Err(PlatformError::ProcessGone { pid });
            }
        }
        Ok(())
    }

    fn fs_free_bytes(&self, path: &Path) -> Result<u64, PlatformError> {
        // GetDiskFreeSpaceExW answers for the volume containing `path`;
        // the caller only needs a sane, unprivileged free-space number.
        let mut wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut free: u64 = 0;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_mut_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(free)
    }

    fn sample_process(&self, pid: u32) -> Option<ProcessSample> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut creation = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut exit = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut kernel = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut user = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let ok = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);

            let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            let memory_ok = GetProcessMemoryInfo(process, &mut counters, counters.cb);
            CloseHandle(process);
            if ok == 0 {
                return None;
            }

            // FILETIME durations are 100 ns units; Duration holds 100 ns
            // exactly, so the conversion never rounds.
            let ticks_100ns = filetime_u64(&kernel).saturating_add(filetime_u64(&user));
            let cpu_time = Duration::from_nanos(ticks_100ns.saturating_mul(100));
            let rss_bytes = if memory_ok != 0 {
                Some(counters.WorkingSetSize as u64)
            } else {
                None
            };
            Some(ProcessSample {
                cpu_time,
                rss_bytes,
            })
        }
    }
}

#[cfg(test)]
mod cpu_rate_tests {
    use super::cpu_rate_from_percent;

    #[test]
    fn one_core_on_a_single_core_machine_is_the_whole_machine() {
        // The tests run on whatever host CI provides; the law is checked
        // relative to the host's own core count: P% of one core must map
        // to 100·P/N whole-machine units, clamped to 1..=10000.
        let cores = super::logical_processors();
        let rate = cpu_rate_from_percent(100);
        let expected = ((100u64 * 100) / cores as u64).clamp(1, 10_000) as u32;
        assert_eq!(rate, expected);
    }

    #[test]
    fn four_cores_never_exceed_the_whole_machine_scale() {
        let rate = cpu_rate_from_percent(400);
        assert!((1..=10_000).contains(&rate));
        let cores = super::logical_processors() as u64;
        if cores >= 4 {
            // On a host with at least four cores, 400% of one core is a
            // quarter... 100·400/N ≤ 10000 must hold and match exactly.
            assert_eq!(rate, ((400u64 * 100) / cores).clamp(1, 10_000) as u32);
        } else {
            // Fewer cores than requested: the ceiling saturates at the
            // whole machine rather than being refused — a cap that asks
            // for more than exists still caps.
            assert_eq!(rate, 10_000);
        }
    }

    #[test]
    fn a_fractional_small_cap_clamps_to_one() {
        // 10% of one core on a 64-core host: 100·10/64 = 15 (fine); on a
        // machine with > 1000 cores it would floor at 1 — the scheduler's
        // smallest unit — never zero (zero would disable the cap).
        assert!(cpu_rate_from_percent(10) >= 1);
    }
}
