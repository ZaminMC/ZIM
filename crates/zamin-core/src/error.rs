//! Core error taxonomy. Internal errors convert to protocol error codes at
//! the IPC boundary; the mapping lives in `zamind`, keeping this crate free
//! of transport concerns.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("server id {id:?} is invalid: {reason}")]
    InvalidServerId { id: String, reason: String },

    #[error("server {id:?} is not registered")]
    ServerNotRegistered { id: String },

    #[error("server {id:?} is already registered")]
    ServerAlreadyRegistered { id: String },

    #[error("directory {path:?} is already registered as another server")]
    ServerRootAlreadyRegistered { path: PathBuf },

    #[error("path {path:?} escapes the server root")]
    PathEscapesRoot { path: PathBuf },

    #[error("path {path:?} is not inside any server root")]
    OutsideRoot { path: PathBuf },

    #[error("path {path:?} does not exist")]
    NotFound { path: PathBuf },

    #[error("path {path:?} is not writable by the current user")]
    NotWritable { path: PathBuf },

    #[error("file {path:?} is {size} bytes, above the {max_bytes} byte read limit")]
    ReadTooLarge {
        path: PathBuf,
        size: u64,
        max_bytes: u64,
    },

    #[error("io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("registry is corrupt at {path:?}: {reason}")]
    RegistryCorrupt { path: PathBuf, reason: String },

    #[error("state file {path:?} has schema version {found}, expected {expected}")]
    SchemaVersion {
        path: PathBuf,
        found: u32,
        expected: u32,
    },

    #[error("java runtime at {path:?} could not be inspected: {reason}")]
    JavaInspectFailed { path: PathBuf, reason: String },

    #[error("no compatible java runtime found for requirement {requirement:?}")]
    JavaNotFound { requirement: String },

    #[error("port {port} is already in use")]
    PortInUse { port: u16 },

    #[error(transparent)]
    Platform(#[from] PlatformError),
}

/// Errors surfaced by the platform seam.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("process {pid} exited or no longer exists")]
    ProcessGone { pid: u32 },

    #[error("process identity mismatch for pid {pid}: expected {expected}, found {found}")]
    IdentityMismatch {
        pid: u32,
        expected: String,
        found: String,
    },

    #[error("graceful os-level signal is not available for this process")]
    GracefulSignalUnsupported,

    #[error("process {program:?} did not finish within {}s", timeout.as_secs())]
    RunTimedOut { program: PathBuf, timeout: Duration },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
