//! Unix domain socket transport (ADR-0008 seam, zamin-ipc side).
//!
//! Daemon ownership is ATOMIC, not probed: the endpoint's lock file
//! (`<socket>.lock`) is held under an exclusive `flock` for the server's
//! whole lifetime. `flock` is decided by the kernel in one step — two
//! daemons racing the bind cannot both win, no matter how their startup
//! interleaves. The probe/delete/bind dance this used to run had a
//! TOCTOU hole instead: A and B both probed a dead socket, both removed
//! it, both bound — two live daemons on one name, and a third daemon
//! could lose its socket out from under it between its own probe and
//! remove. The lock file is never unlinked (an unlink would hand the
//! name to a process holding the dead inode); it simply stays, one tiny
//! empty file, exactly like the named pipe's kernel-side ownership on
//! Windows (`first_pipe_instance`).

use std::fs::{File, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::net::{UnixListener, UnixStream};

use crate::connection::Connection;
use crate::endpoint::Endpoint;
use crate::error::IpcError;

pub struct PlatformServer {
    path: PathBuf,
    listener: UnixListener,
    /// The ownership proof, held open (and therefore locked) until drop.
    /// Dropping the file releases the flock — a dead daemon's OS cleanup
    /// is the recovery path, which is precisely why no stale lock can
    /// wedge the endpoint shut.
    _lock: File,
    /// Guards the Drop cleanup against double-run (Drop + explicit).
    cleaned: AtomicBool,
}

impl Drop for PlatformServer {
    fn drop(&mut self) {
        // Remove OUR socket file on a controlled exit. The lock file
        // stays (see the module doc — unlinking it would break the very
        // atomicity the lock exists for). A SIGKILLed daemon leaves the
        // socket behind; the next bind reclaims it safely UNDER the
        // flock it will then own.
        if !self.cleaned.swap(true, Ordering::SeqCst) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Take the exclusive flock on `path`'s lock file, or fail with
/// [`IpcError::AlreadyRunning`] when a live daemon owns it. The kernel
/// releases the lock if the owner dies, so the only way this fails is
/// "another live process holds it" — there is no stale case.
fn acquire_lock(path: &std::path::Path) -> Result<File, IpcError> {
    use std::os::unix::io::AsRawFd;
    let lock_path = lock_path_for(path);
    let file = File::create(&lock_path)?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return Err(IpcError::AlreadyRunning);
    }
    Ok(file)
}

fn lock_path_for(socket_path: &std::path::Path) -> PathBuf {
    // `zamind.sock` -> `zamind.lock` (the extension swap keeps the pair
    // visually paired in `ls`; the name is otherwise arbitrary).
    let mut owned = socket_path.as_os_str().to_owned();
    owned.push(".lock");
    PathBuf::from(owned)
}

impl PlatformServer {
    pub async fn bind(endpoint: Endpoint) -> Result<PlatformServer, IpcError> {
        let path = match endpoint {
            Endpoint::UnixSocket(path) => path,
            other => {
                return Err(IpcError::EndpointInvalid(format!(
                    "{other:?} is not a Unix endpoint"
                )))
            }
        };

        if let Some(parent) = path.parent() {
            // Restrict permissions only when the daemon creates the
            // directory itself (the default endpoint's per-user runtime
            // dir). An operator-supplied --endpoint may live in a shared
            // parent such as /tmp; chmodding that to 0700 would break
            // everything else in it.
            let created = std::fs::metadata(parent).is_err();
            std::fs::create_dir_all(parent)?;
            if created {
                std::fs::set_permissions(parent, Permissions::from_mode(0o700))?;
            }
        }

        // THE OWNERSHIP STEP, first and atomic: whoever holds the flock
        // owns the endpoint's name. Everything below runs under it.
        let lock = acquire_lock(&path)?;

        // We own the name, so anything at the socket path is by
        // definition a leftover of a dead process (a live daemon would
        // still hold the lock). Reclaim it — socket file or not.
        if path.exists() {
            std::fs::remove_file(&path)?;
        }

        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, Permissions::from_mode(0o600))?;

        Ok(PlatformServer {
            path,
            listener,
            _lock: lock,
            cleaned: AtomicBool::new(false),
        })
    }

    pub async fn accept(&mut self) -> Result<Connection, IpcError> {
        let (stream, _addr) = self.listener.accept().await?;
        Ok(Connection::new(stream))
    }
}

pub async fn connect(endpoint: Endpoint) -> Result<Connection, IpcError> {
    let path = match endpoint {
        Endpoint::UnixSocket(path) => path,
        other => {
            return Err(IpcError::EndpointInvalid(format!(
                "{other:?} is not a Unix endpoint"
            )))
        }
    };
    match UnixStream::connect(&path).await {
        Ok(stream) => Ok(Connection::new(stream)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(IpcError::NoDaemon),
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => Err(IpcError::NoDaemon),
        Err(e) => Err(e.into()),
    }
}
