//! Windows process operations: hidden-window spawn into a new process
//! group and a Job Object (no kill-on-close — the daemon dying must never
//! kill servers, ADR-0001), creation-time process identity, CTRL_BREAK
//! graceful signal, and job-based tree termination.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};

use crate::error::PlatformError;
use crate::platform::{ProcessIdentity, ProcessOps, SpawnHandle, SpawnSpec, Spawned};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0200;

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

struct WindowsHandle {
    pid: u32,
    child: tokio::process::Child,
    job: Option<JobHandle>,
}

impl SpawnHandle for WindowsHandle {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn child(&mut self) -> &mut tokio::process::Child {
        &mut self.child
    }

    fn force_kill_tree(&mut self) -> Result<(), PlatformError> {
        if let Some(job) = &self.job {
            if unsafe { TerminateJobObject(job.0, 1) } != 0 {
                return Ok(());
            }
        }
        self.child
            .start_kill()
            .map_err(|_| PlatformError::ProcessGone { pid: self.pid })
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

        // The job object is the tree-kill mechanism. Without kill-on-close,
        // the job outlives the daemon and adoption (ADR-0005) still sees a
        // live process.
        let job: Option<JobHandle> = child.raw_handle().and_then(|process_handle| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            if AssignProcessToJobObject(job, process_handle) == 0 {
                CloseHandle(job);
                return None;
            }
            Some(JobHandle(job))
        });

        Ok(Spawned::new(Box::new(WindowsHandle { pid, child, job })))
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
}
