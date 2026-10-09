//! The Windows OS sandbox (Part 2): a real process boundary, not a path
//! check. Each managed server gets its own AppContainer — a kernel-enforced
//! access model where the process can touch ONLY objects whose ACLs grant
//! its container SID (or a capability SID it was created with):
//!
//! - The server directory is ACL'd for the container SID (the jail).
//! - User profile, other servers' directories, the daemon's state, the
//!   panel's files: their ACLs name no AppContainer → the server process
//!   is refused at the kernel, however it spells the path (native APIs,
//!   symlinks, junctions — the check runs on the final object, not on a
//!   string).
//! - Outbound network follows the granted capability SIDs (resolved by
//!   the OS from the documented capability names — no hand-typed SID
//!   arithmetic): `unrestricted` grants the internet-client pair,
//!   `local-only` grants none but exempts the container from the
//!   loopback block, `blocked` grants none and exempts nothing.
//!   Inbound Minecraft clients are NOT affected — the container model
//!   gates the container's own outbound connects; the server's listening
//!   socket accepts clients exactly as before.
//! - Program Files / System32 keep their default ALL APPLICATION
//!   PACKAGES read+execute ACEs, so the JVM itself loads and runs.
//!   Nothing else is implied by that grant: read+execute of system
//!   binaries, never write, and no path into user data.
//!
//! What this module is NOT, stated honestly:
//! - It does not stop the server from READING world-readable system
//!   binaries (the JVM must load); it stops user data access.
//! - It does not observe refused in-process connects (no filter driver);
//!   blocked outbound manifests as connection errors inside the server's
//!   own logs.
//! - Adopted servers (spawned by an older daemon before this boundary
//!   existed) cannot be retro-fitted into a container; the inspector
//!   says so.

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, SetHandleInformation, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GENERIC_ALL,
    HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW,
    EXPLICIT_ACCESS_W, GRANT_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{
    DeriveCapabilitySidsFromName, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES,
    SUB_CONTAINERS_AND_OBJECTS_INHERIT,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::error::PlatformError;
use crate::platform::{JobNotice, NetworkSandbox, SpawnSpec};

/// Creation flags for the sandboxed spawn. CREATE_SUSPENDED holds the
/// process still while the Job Object is assigned, so the enforcement
/// boundary is in place before the JVM executes a single instruction.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0200;
const CREATE_SUSPENDED: u32 = 0x0000_0040;
const EXTENDED_STARTUPINFO: u32 = 0x0008_0000; // EXTENDED_STARTUPINFO_PRESENT (winbase.h)

/// Job-object message codes (winbase.h): the watcher translates ONLY
/// refusals the OS actually posted — everything else is ignored, and no
/// notice is ever synthesized.
const JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT: usize = 3;
const JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT: usize = 9;
const JOB_OBJECT_MSG_JOB_MEMORY_LIMIT: usize = 10;

/// A container SID + the OS string derived from it. The SID is kept for
/// the daemon's lifetime (allocated by the OS; per-server SIDs are few
/// and the daemon is long-lived — freeing cross-API allocations wrongly
/// would be worse than never freeing).
pub struct Container {
    pub sid: windows_sys::Win32::Security::PSID,
    /// S-1-... string form, for the loopback-exemption command line.
    pub sid_string: String,
}

unsafe impl Send for Container {}
unsafe impl Sync for Container {}

/// The profile name grammar: CreateAppContainerProfile accepts only
/// alphanumeric characters and periods, ≤ 64 chars. Server ids are
/// already restricted at registration; the prefix is static.
pub fn container_profile_name(server_id: &str) -> String {
    let cleaned: String = server_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.')
        .take(52)
        .collect();
    format!("zim.server.{cleaned}")
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_os_error(what: &str) -> PlatformError {
    let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    PlatformError::SandboxBuild {
        detail: format!("{what} failed (win32 error {code})"),
    }
}

fn hresult_err(what: &str, hr: i32) -> PlatformError {
    PlatformError::SandboxBuild {
        detail: format!("{what} failed (hresult 0x{:08x})", hr as u32),
    }
}

/// Create (or look up) the server's AppContainer profile and return its
/// SID. Idempotent by design: the profile persists across starts so a
/// server keeps one identity for its whole life inside the daemon.
pub fn ensure_container(name: &str) -> Result<Container, PlatformError> {
    let name_w = wide(name);
    let display = wide("ZIM server sandbox");
    let desc = wide("ZIM-managed Minecraft server process boundary");

    let mut sid: windows_sys::Win32::Security::PSID = std::ptr::null_mut();
    let hr = unsafe {
        CreateAppContainerProfile(
            name_w.as_ptr(),
            display.as_ptr(),
            desc.as_ptr(),
            std::ptr::null(),
            0,
            &mut sid,
        )
    };
    if hr == ERROR_SUCCESS as i32 {
        return unsafe { finish_container(sid) };
    }
    // The profile already exists (a previous start created it): reuse it.
    if hr as u32 == ERROR_ALREADY_EXISTS {
        let hr2 = unsafe { DeriveAppContainerSidFromAppContainerName(name_w.as_ptr(), &mut sid) };
        if hr2 == ERROR_SUCCESS as i32 {
            return unsafe { finish_container(sid) };
        }
        return Err(hresult_err("looking up the existing sandbox profile", hr2));
    }
    Err(hresult_err("creating the sandbox profile", hr))
}

unsafe fn finish_container(
    sid: windows_sys::Win32::Security::PSID,
) -> Result<Container, PlatformError> {
    let mut string: windows_sys::core::PWSTR = std::ptr::null_mut();
    if ConvertSidToStringSidW(sid, &mut string) == 0 {
        return Err(last_os_error("stringifying the sandbox SID"));
    }
    // The string is allocated by the OS; copy it, then free it.
    let mut len = 0usize;
    while *string.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(string, len);
    let sid_string = String::from_utf16_lossy(slice);
    unsafe { LocalFree(string as _) };
    Ok(Container { sid, sid_string })
}

/// Grant the container SID full control of the server directory tree.
/// This is the jail's grant side; every OTHER directory's existing ACL
/// (which names no AppContainer) is the deny side — no ACE is added to
/// anything outside `dir`. Idempotent: SetEntriesInAclW merges, and a
/// repeated identical grant collapses onto the same ACE.
pub fn grant_tree_access(
    dir: &Path,
    sid: windows_sys::Win32::Security::PSID,
) -> Result<(), PlatformError> {
    let dir_w = wide(&dir.as_os_str().to_string_lossy());
    let mut dacl: *mut windows_sys::Win32::Security::ACL = std::ptr::null_mut();
    let ok = unsafe {
        GetNamedSecurityInfoW(
            dir_w.as_ptr(),
            SE_FILE_OBJECT,
            windows_sys::Win32::Security::DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok != ERROR_SUCCESS {
        return Err(PlatformError::SandboxBuild {
            detail: format!("reading the directory ACL failed (win32 error {ok})"),
        });
    }

    let trustee = TRUSTEE_W {
        MultipleTrusteeOperation: 0,
        TrusteeForm: TRUSTEE_IS_SID,
        TrusteeType: TRUSTEE_IS_UNKNOWN,
        pMultipleTrustee: std::ptr::null_mut(),
        ptstrName: sid as *mut u16,
    };
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: trustee,
    };
    let mut new_dacl: *mut windows_sys::Win32::Security::ACL = std::ptr::null_mut();
    let ok = unsafe { SetEntriesInAclW(1, &entry, dacl, &mut new_dacl) };
    if ok != ERROR_SUCCESS {
        return Err(PlatformError::SandboxBuild {
            detail: format!("building the sandbox ACE failed (win32 error {ok})"),
        });
    }
    let ok = unsafe {
        SetNamedSecurityInfoW(
            dir_w.as_ptr(),
            SE_FILE_OBJECT,
            windows_sys::Win32::Security::DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            new_dacl,
            std::ptr::null_mut(),
        )
    };
    unsafe { LocalFree(new_dacl as _) };
    if ok != ERROR_SUCCESS {
        return Err(PlatformError::SandboxBuild {
            detail: format!("applying the sandbox ACL failed (win32 error {ok})"),
        });
    }
    Ok(())
}

/// Resolve the policy's capability SIDs through the OS. The returned
/// SID_AND_ATTRIBUTES point into OS allocations that stay alive for the
/// daemon's lifetime (same policy as `Container`): the spawn path reads
/// them after this function returns.
fn policy_capabilities(network: NetworkSandbox) -> Vec<SID_AND_ATTRIBUTES> {
    let names: &[&str] = match network {
        NetworkSandbox::Unrestricted => &["internetClient", "internetClientServer"],
        NetworkSandbox::LocalOnly | NetworkSandbox::BlockedOutbound => &[],
    };
    let mut out = Vec::new();
    for name in names {
        let name_w = wide(name);
        let mut group_sids: *mut windows_sys::Win32::Security::PSID = std::ptr::null_mut();
        let mut group_count = 0u32;
        let mut sids: *mut windows_sys::Win32::Security::PSID = std::ptr::null_mut();
        let mut sid_count = 0u32;
        let ok = unsafe {
            DeriveCapabilitySidsFromName(
                name_w.as_ptr(),
                &mut group_sids,
                &mut group_count,
                &mut sids,
                &mut sid_count,
            )
        };
        if ok != 0 {
            for i in 0..sid_count as usize {
                let p = unsafe { *sids.add(i) };
                out.push(SID_AND_ATTRIBUTES {
                    Sid: p,
                    Attributes: 0,
                });
            }
        }
        // A failed derivation degrades to "fewer capabilities granted",
        // never to "more": LocalOnly/BlockedOutbound grant none at all,
        // and Unrestricted missing one capability narrows the server's
        // own outbound — observable, honest, and safe in both directions.
    }
    out
}

/// Exempt the container from the default loopback block (local-only).
/// `CheckNetIsolation.exe` is the documented, unprivileged path; the
/// exemption is per-user machine state that outlives processes, so the
/// matching remove runs at server removal (see `cleanup_container`).
fn add_loopback_exemption(sid_string: &str) {
    let _ = std::process::Command::new("checknetisolation")
        .args(["loopbackexempt", "-a", "-p", sid_string])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

pub fn remove_loopback_exemption(sid_string: &str) {
    let _ = std::process::Command::new("checknetisolation")
        .args(["loopbackexempt", "-d", "-p", sid_string])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

/// Delete the server's AppContainer profile (server removal). Self-
/// contained: the container's SID is re-derived from the profile name so
/// the loopback exemption added at spawn is removed here without the
/// daemon keeping a registry of SID strings.
pub fn cleanup_container(name: &str) {
    let name_w = wide(name);
    let mut sid: windows_sys::Win32::Security::PSID = std::ptr::null_mut();
    if unsafe { DeriveAppContainerSidFromAppContainerName(name_w.as_ptr(), &mut sid) }
        == ERROR_SUCCESS as i32
    {
        let mut string: windows_sys::core::PWSTR = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut string) } != 0 {
            unsafe {
                let mut len = 0usize;
                while *string.add(len) != 0 {
                    len += 1;
                }
                let sid_string = String::from_utf16_lossy(std::slice::from_raw_parts(string, len));
                remove_loopback_exemption(&sid_string);
            }
        }
    }
    // The profile delete is the boundary's retirement; a failure leaves
    // an orphaned (inert) profile — never fatal.
    unsafe { DeleteAppContainerProfile(name_w.as_ptr()) };
}

/// One pipe pair for the child's stdio: `(ours, child's)`. The child's
/// end is inheritable; ours is explicitly de-inherited so the sandboxed
/// process cannot reach the daemon's own ends through handle scanning.
fn make_pipe(child_reads: bool) -> Result<(OwnedHandle, OwnedHandle), PlatformError> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, &sa, 0) } == 0 {
        return Err(last_os_error("creating the stdio pipe"));
    }
    let (ours, theirs) = if child_reads {
        (write, read)
    } else {
        (read, write)
    };
    // Our end must not leak into the sandboxed process.
    unsafe { SetHandleInformation(ours, HANDLE_FLAG_INHERIT, 0) };
    Ok((
        // SAFETY: each handle was just created by CreatePipe and is
        // uniquely owned here.
        unsafe { OwnedHandle::from_raw_handle(ours) },
        unsafe { OwnedHandle::from_raw_handle(theirs) },
    ))
}

/// Quote one argument for a raw command line (the mirror of the CRT's
/// parse): wrap args containing space/quote/tab, double inner quotes,
/// and keep trailing backslashes before a closing quote intact.
fn quote_arg(arg: &str) -> String {
    let needs_quotes = arg.is_empty() || arg.chars().any(|c| c == ' ' || c == '\t' || c == '"');
    if !needs_quotes {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 8);
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Backslashes preceding a quote double up, then the quote.
                out.push_str(&"\\".repeat(backslashes + 1));
                out.push('"');
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    // Backslashes before the closing quote must be doubled.
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// A raw-spawned (sandboxed) child: the raw OS pipe ends (bridged to
/// async by `bridge_reader`/`bridge_writer` at the trait's take_* calls)
/// plus the process and MAIN THREAD handles. The thread stays SUSPENDED:
/// the caller assigns the enforcement Job Object first, then resumes —
/// zero instructions run before the boundary is in place.
pub struct RawChild {
    pub stdin: OwnedHandle,
    pub stdout: OwnedHandle,
    pub stderr: OwnedHandle,
    pub process: OwnedHandle,
    pub thread: OwnedHandle,
}

/// Bridge a raw pipe's read end into the actor's async world: a blocking
/// thread reads the pipe; chunks cross an mpsc; a hand-written
/// `AsyncRead` polls the channel (`Receiver::poll_recv` is public for
/// exactly this). Windows std has no `from_raw_handle` for its child
/// stream types, so this is the sanctioned bridge — the thread ends when
/// the reader is dropped (sends fail and it exits).
pub fn bridge_reader(raw: OwnedHandle) -> Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin> {
    use std::io::Read;
    let mut file = std::fs::File::from(raw);
    let (tx, rx) = tokio::sync::mpsc::channel::<std::io::Result<bytes::Bytes>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx
                        .blocking_send(Ok(bytes::Bytes::copy_from_slice(&buf[..n])))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.blocking_send(Err(e));
                    break;
                }
            }
        }
    });
    Box::new(ReaderBridge { rx })
}

struct ReaderBridge {
    rx: tokio::sync::mpsc::Receiver<std::io::Result<bytes::Bytes>>,
}

impl tokio::io::AsyncRead for ReaderBridge {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let self_mut = self.get_mut();
        match self_mut.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                buf.put_slice(&chunk);
                Poll::Ready(Ok(()))
            }
            // The thread finished: an honest EOF, not an error.
            Poll::Ready(None) => Poll::Ready(Ok(())),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// The write-end twin: `PollSender` bridges the async poll model onto
/// the channel; a blocking thread drains it and writes the pipe. The
/// actor's `write_all` + `flush` pattern delivers each console command.
pub fn bridge_writer(raw: OwnedHandle) -> Box<dyn tokio::io::AsyncWrite + Send + Sync + Unpin> {
    use std::io::Write;
    let mut file = std::fs::File::from(raw);
    let (tx, rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(16);
    std::thread::spawn(move || {
        let mut rx = rx;
        while let Some(chunk) = rx.blocking_recv() {
            if file.write_all(&chunk).is_err() {
                break;
            }
            let _ = file.flush();
        }
    });
    Box::new(WriterBridge {
        sender: tokio_util::sync::PollSender::new(tx),
    })
}

struct WriterBridge {
    sender: tokio_util::sync::PollSender<bytes::Bytes>,
}

impl tokio::io::AsyncWrite for WriterBridge {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.sender.poll_reserve(cx) {
            Poll::Ready(Ok(())) => {
                let _ = self.sender.send_item(bytes::Bytes::copy_from_slice(buf));
                Poll::Ready(Ok(buf.len()))
            }
            Poll::Ready(Err(_)) => Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the console writer thread is gone",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        // The writer thread flushes after every chunk it writes.
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        // Dropping the sender ends the writer thread, which flushes the
        // file before exiting.
        self.sender.close();
        Poll::Ready(Ok(()))
    }
}

pub fn spawn_appcontainer(
    spec: &SpawnSpec,
    container: &Container,
    network: NetworkSandbox,
) -> Result<RawChild, PlatformError> {
    // 1. The jail's grant: the tree gets its container ACE.
    grant_tree_access(&spec.working_dir, container.sid)?;

    // 2. The network policy's one piece of machine state: local-only
    //    needs the container exempted from the default loopback block
    //    (inbound players are unaffected either way; this is about the
    //    server's own outbound to LOCAL services). BlockedOutbound and
    //    Unrestricted need nothing here.
    if network == NetworkSandbox::LocalOnly {
        add_loopback_exemption(&container.sid_string);
    }

    // 2. Stdio pipes: the JVM's console lives in the log pipeline.
    let (stdin_ours, stdin_child) = make_pipe(true)?;
    let (stdout_ours, stdout_child) = make_pipe(false)?;
    let (stderr_ours, stderr_child) = make_pipe(false)?;

    // 3. Command line (raw CreateProcessW wants the whole line, NUL
    //    terminated — `wide` provides the terminator).
    let mut cmdline = wide(
        &spec
            .args
            .iter()
            .map(|a| quote_arg(a))
            .collect::<Vec<_>>()
            .join(" "),
    );

    // 4. Environment: the parent's env with TMP/TEMP aimed INSIDE the
    //    jail (the container cannot write the user's temp dir; the JVM
    //    needs a writable temp or it fails at startup).
    let tmp = spec.working_dir.join("tmp");
    let _ = std::fs::create_dir_all(&tmp);
    let tmp_s = tmp.to_string_lossy().into_owned();
    let mut env_block: Vec<u16> = Vec::new();
    let mut seen_tmp = false;
    let mut seen_temp = false;
    for (key, value) in std::env::vars() {
        let (key, value) = match key.as_str() {
            "TMP" => {
                seen_tmp = true;
                ("TMP".to_owned(), tmp_s.clone())
            }
            "TEMP" => {
                seen_temp = true;
                ("TEMP".to_owned(), tmp_s.clone())
            }
            _ => (key, value),
        };
        env_block.extend(wide(&format!("{key}={value}")));
    }
    if !seen_tmp {
        env_block.extend(wide(&format!("TMP={tmp_s}")));
    }
    if !seen_temp {
        env_block.extend(wide(&format!("TEMP={tmp_s}")));
    }
    env_block.push(0); // the block's own double terminator

    // 5. The capability set rides process creation — a plugin can never
    //    grant more network to itself later.
    let capabilities = policy_capabilities(network);
    let sec_caps = SECURITY_CAPABILITIES {
        AppContainerSid: container.sid,
        Capabilities: if capabilities.is_empty() {
            std::ptr::null_mut()
        } else {
            capabilities.as_ptr() as *mut SID_AND_ATTRIBUTES
        },
        CapabilityCount: capabilities.len() as u32,
        Reserved: 0,
    };

    // 6. The attribute list: one attribute, the security capabilities.
    let mut attr_size = 0usize;
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attr_size) };
    let mut attr_buf: Vec<u8> = vec![0u8; attr_size];
    let attr_list = attr_buf.as_mut_ptr() as *mut c_void;
    if unsafe { InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_size) } == 0 {
        return Err(last_os_error("initializing the process attribute list"));
    }
    if unsafe {
        UpdateProcThreadAttribute(
            attr_list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &sec_caps as *const _ as *const c_void,
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    } == 0
    {
        unsafe { DeleteProcThreadAttributeList(attr_list as _) };
        return Err(last_os_error("applying the sandbox attributes"));
    }

    // 7. STARTUPINFOEXW with the stdio handles.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = stdin_child.as_raw_handle();
    startup.StartupInfo.hStdOutput = stdout_child.as_raw_handle();
    startup.StartupInfo.hStdError = stderr_child.as_raw_handle();
    startup.lpAttributeList = attr_list as _;

    // 8. Create suspended → job assign (the caller) → resume. Suspended
    //    is the race-free ordering: zero instructions run unbounded.
    let program_w = wide(&spec.program.to_string_lossy());
    let workdir_w = wide(&spec.working_dir.to_string_lossy());
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            program_w.as_ptr(),
            cmdline.as_mut_ptr() as *mut u16,
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED | EXTENDED_STARTUPINFO,
            env_block.as_ptr() as *const c_void,
            workdir_w.as_ptr(),
            &startup as *const STARTUPINFOEXW
                as *const windows_sys::Win32::System::Threading::STARTUPINFOW,
            &mut pi,
        )
    };
    // The attribute list dies with this call frame on every path.
    unsafe { DeleteProcThreadAttributeList(attr_list as _) };
    if ok == 0 {
        return Err(last_os_error("spawning the sandboxed server process"));
    }

    // Child-end handles: the child holds its own references; ours close.
    drop(stdin_child);
    drop(stdout_child);
    drop(stderr_child);

    // The main thread stays SUSPENDED — the caller assigns the
    // enforcement Job Object first, then calls `resume` below. Zero
    // instructions run before the boundary is in place.
    Ok(RawChild {
        stdin: stdin_ours,
        stdout: stdout_ours,
        stderr: stderr_ours,
        process: unsafe { OwnedHandle::from_raw_handle(pi.hProcess) },
        thread: unsafe { OwnedHandle::from_raw_handle(pi.hThread) },
    })
}

/// Resume a suspended sandboxed child (after the Job assignment).
pub fn resume(process: &OwnedHandle, thread: &OwnedHandle) -> Result<(), PlatformError> {
    let ret = unsafe { ResumeThread(thread.as_raw_handle()) };
    if ret == u32::MAX {
        // A suspended-forever process is a zombie: terminate it and fail
        // the spawn rather than leave it hanging.
        unsafe { TerminateProcess(process.as_raw_handle(), 1) };
        return Err(last_os_error("resuming the sandboxed process"));
    }
    Ok(())
}

/// Non-blocking exit poll over a raw process handle.
pub fn try_wait_raw(
    process: &OwnedHandle,
) -> Result<Option<std::process::ExitStatus>, PlatformError> {
    use std::os::windows::process::ExitStatusExt;
    let wait = unsafe { WaitForSingleObject(process.as_raw_handle(), 0) };
    if wait == WAIT_TIMEOUT {
        return Ok(None);
    }
    if wait != WAIT_OBJECT_0 {
        return Err(last_os_error("waiting on the sandboxed process"));
    }
    let mut code: u32 = 0;
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return Err(last_os_error("reading the sandboxed process exit code"));
    }
    Ok(Some(std::process::ExitStatus::from_raw(code)))
}

/// Kill the raw child's process (the job does tree-kill; this fallback
/// targets the single process).
pub fn kill_raw(process: &OwnedHandle) -> Result<(), PlatformError> {
    if unsafe { TerminateProcess(process.as_raw_handle(), 1) } == 0 {
        return Err(last_os_error("terminating the sandboxed process"));
    }
    Ok(())
}

/// The Job Object's completion-port watcher: the OS posts a message for
/// each boundary event (process-limit refusals, memory-limit refusals).
/// One thread per spawn; it ends when the daemon drops the receiver
/// (the channel's send fails and the loop exits) and closes the port.
pub fn spawn_job_watcher(
    job: HANDLE,
    tx: std::sync::mpsc::Sender<JobNotice>,
) -> Result<(), PlatformError> {
    let port = unsafe {
        windows_sys::Win32::System::IO::CreateIoCompletionPort(
            INVALID_HANDLE_VALUE,
            std::ptr::null_mut(),
            0,
            1,
        )
    };
    if port.is_null() {
        return Err(last_os_error("creating the job notification port"));
    }
    let assoc = windows_sys::Win32::System::JobObjects::JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
        CompletionKey: std::ptr::null_mut(),
        CompletionPort: port,
    };
    let ok = unsafe {
        windows_sys::Win32::System::JobObjects::SetInformationJobObject(
            job,
            windows_sys::Win32::System::JobObjects::JobObjectAssociateCompletionPortInformation,
            &assoc as *const _ as *const c_void,
            std::mem::size_of::<
                windows_sys::Win32::System::JobObjects::JOBOBJECT_ASSOCIATE_COMPLETION_PORT,
            >() as u32,
        )
    };
    if ok == 0 {
        unsafe { CloseHandle(port) };
        return Err(last_os_error("associating the job notification port"));
    }

    struct PortHandle(HANDLE);
    unsafe impl Send for PortHandle {}
    let port = PortHandle(port);
    std::thread::spawn(move || unsafe {
        // Capture the WHOLE wrapper: edition 2021's disjoint capture
        // would otherwise take just `port.0` (the raw pointer field),
        // and Send is checked on the capture, not the wrapper type.
        let port = port;
        let mut bytes: u32 = 0;
        let mut key: usize = 0;
        let mut overlapped: *mut windows_sys::Win32::System::IO::OVERLAPPED = std::ptr::null_mut();
        loop {
            let ok = windows_sys::Win32::System::IO::GetQueuedCompletionStatus(
                port.0,
                &mut bytes,
                &mut key,
                &mut overlapped,
                u32::MAX,
            );
            if ok == 0 {
                // Port closed (daemon shutdown): end the watcher.
                break;
            }
            // Job notifications arrive with the MESSAGE as the completion
            // key and the relevant pid in the byte count (winbase.h).
            let notice = match key {
                JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT => {
                    Some(JobNotice::ActiveProcessLimit { pid: bytes })
                }
                JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT => {
                    Some(JobNotice::ProcessMemoryLimit { pid: bytes })
                }
                JOB_OBJECT_MSG_JOB_MEMORY_LIMIT => Some(JobNotice::JobMemoryLimit),
                _ => None,
            };
            if let Some(notice) = notice {
                if tx.send(notice).is_err() {
                    break; // the daemon dropped the receiver
                }
            }
        }
        CloseHandle(port.0);
    });
    Ok(())
}

// The adversarial boundary tests (Part 2, Testing requirements). They run
// ONLY on Windows (the boundary is Windows-native) and are marked
// #[ignore] so the ordinary `cargo test` sweep stays hermetic; the
// Windows CI job runs them with `cargo test -- --ignored`. They test the
// REAL boundary — a live sandboxed process attempting escapes — not a
// Rust path-validation helper.
#[cfg(all(test, windows))]
mod adversarial {
    use super::*;
    use crate::platform::SandboxSpawn;
    use std::path::PathBuf;

    /// Spawn a one-shot command inside the container and return
    /// (exit_code, stdout). Used by every escape attempt below.
    fn run_in_sandbox(dir: &Path, program: &str, args: &[&str]) -> (i32, String) {
        let sandbox = SandboxSpawn {
            container_name: format!("zim.test.{}", std::process::id()),
            network: NetworkSandbox::BlockedOutbound,
        };
        let spec = SpawnSpec {
            program: PathBuf::from(program),
            args: args.iter().map(|a| a.to_string()).collect(),
            working_dir: dir.to_path_buf(),
            limits: Default::default(),
            sandbox: Some(sandbox),
        };
        let mut spawned = crate::platform::process()
            .spawn(&spec)
            .expect("the sandboxed spawn must succeed on Windows");
        let handle = spawned.handle();
        drop(handle.take_stdin());
        let mut stdout = handle.take_stdout().expect("stdout piped").unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime builds");
        let mut output = String::new();
        rt.block_on(async {
            tokio::io::AsyncReadExt::read_to_string(&mut stdout, &mut output)
                .await
                .expect("stdout reads");
        });
        let _ = handle.take_stderr();
        let mut status = None;
        for _ in 0..600 {
            match spawned.handle().try_wait() {
                Ok(Some(s)) => {
                    status = Some(s);
                    break;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(_) => break,
            }
        }
        let status = status.expect("a one-shot command finishes within 30 s");
        (status.code().unwrap_or(-1), output)
    }

    fn jail_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zim-jail-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("jail dir creates");
        dir
    }

    #[test]
    #[ignore]
    fn writing_inside_the_jail_succeeds() {
        let dir = jail_dir("in");
        let (code, _out) = run_in_sandbox(&dir, "cmd.exe", &["/C", "echo ok> inside.txt"]);
        assert_eq!(code, 0, "in-jail writes succeed");
        assert!(dir.join("inside.txt").exists(), "the file landed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore]
    fn reading_the_user_profile_is_refused_at_the_kernel() {
        let dir = jail_dir("profile");
        let home = std::env::var("USERPROFILE").expect("a user profile exists");
        let probe = PathBuf::from(&home).join("ntuser.ini");
        // Any file outside the jail answers the same: ACCESS DENIED. The
        // probe target need not exist — the ACL check refuses before the
        // filesystem ever resolves the name.
        let script = format!(
            "type \"{}\" >nul 2>&1 & if errorlevel 1 (exit /b 7) else (exit /b 0)",
            probe.to_string_lossy()
        );
        let (code, _out) = run_in_sandbox(&dir, "cmd.exe", &["/C", &script]);
        assert_eq!(code, 7, "reading outside the jail is refused");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore]
    fn a_sibling_server_directory_is_refused() {
        let dir = jail_dir("sibling");
        let other = jail_dir("other-server");
        std::fs::write(other.join("secret.txt"), "faction data").expect("sibling file writes");
        let script = format!(
            "type \"{}\" >nul 2>&1 & if errorlevel 1 (exit /b 7) else (exit /b 0)",
            other.join("secret.txt").to_string_lossy()
        );
        let (code, _out) = run_in_sandbox(&dir, "cmd.exe", &["/C", &script]);
        assert_eq!(code, 7, "cross-server reads are refused at the kernel");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    #[ignore]
    fn a_junction_escape_is_refused() {
        // A junction INSIDE the jail pointing OUT: the ACL check runs on
        // the final object, so the escape lands on the outside ACL and is
        // refused — a string check could never do this.
        let dir = jail_dir("junction");
        let outside = jail_dir("outside");
        let link = dir.join("escape");
        let _ = std::process::Command::new("cmd.exe")
            .args([
                "/C",
                &format!(
                    "mklink /J \"{}\" \"{}\"",
                    link.to_string_lossy(),
                    outside.to_string_lossy()
                ),
            ])
            .status()
            .expect("junction creation runs");
        let script = format!(
            "echo smuggled> \"{}\" & if errorlevel 1 (exit /b 7) else (exit /b 0)",
            link.join("file.txt").to_string_lossy()
        );
        let (code, _out) = run_in_sandbox(&dir, "cmd.exe", &["/C", &script]);
        assert_eq!(code, 7, "a junction out of the jail is refused");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    #[ignore]
    fn blocked_outbound_refuses_remote_connects() {
        // The capability set was granted at process creation; a plugin
        // that dials out gets the connection refused by the OS, observed
        // as PowerShell's non-zero exit for a remote TCP attempt.
        let dir = jail_dir("net");
        let script = "$c = New-Object Net.Sockets.TcpClient; \
                      try { $i = $c.BeginConnect('93.184.216.34', 80, $null, $null); \
                      if ($i.AsyncWaitHandle.WaitOne(3000) -and $c.Connected) { exit 0 } else { exit 7 } } \
                      finally { $c.Close() }";
        let (code, _out) =
            run_in_sandbox(&dir, "powershell.exe", &["-NoProfile", "-Command", script]);
        assert_eq!(code, 7, "blocked outbound refuses remote connects");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
