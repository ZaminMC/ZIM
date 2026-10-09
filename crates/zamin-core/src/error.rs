//! Core error taxonomy. Internal errors convert to protocol error codes at
//! the IPC boundary; the mapping lives in `zamind`, keeping this crate free
//! of transport concerns.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("server id {id:?} is invalid: {reason}")]
    InvalidServerId { id: String, reason: String },

    #[error("the server requires EULA acceptance at {path:?}")]
    NeedsEula { path: PathBuf },

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

    #[error("copy target {path:?} already exists; copies never overwrite")]
    CopyTargetExists { path: PathBuf },

    #[error(
        "copy of {path:?} is {size} bytes, above the {max_bytes} byte limit for one synchronous copy"
    )]
    CopyTooLarge {
        path: PathBuf,
        size: u64,
        max_bytes: u64,
    },

    #[error("directory tree below {path:?} is deeper than the copy depth limit")]
    CopyTooDeep { path: PathBuf },

    #[error("refusing to copy through the symlink at {path:?}; copy its real target instead")]
    SymlinkInCopy { path: PathBuf },

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

    #[error("java major {found} does not satisfy the required major {required}")]
    JavaIncompatible { found: u32, required: u32 },

    #[error("only {available_mb} MiB free at {path:?}; at least {required_mb} MiB is required")]
    InsufficientDisk {
        path: PathBuf,
        available_mb: u64,
        required_mb: u64,
    },

    #[error("port {port} is already in use")]
    PortInUse { port: u16 },

    #[error("setting {field:?} is invalid: {reason}")]
    ConfigInvalid { field: String, reason: String },

    #[error("archive entry {entry:?} is unsafe: {reason}")]
    ArchiveUnsafeEntry { entry: String, reason: String },

    #[error("archive exceeds safety limits: {found} bytes across {entries} entries, max {max_bytes} bytes / {max_entries} entries")]
    ArchiveTooLarge {
        found: u64,
        entries: u64,
        max_bytes: u64,
        max_entries: u64,
    },

    #[error("the disk is full at {path:?}")]
    DiskFull { path: PathBuf },

    #[error("the operation was cancelled")]
    Cancelled,

    #[error("the server at {url} answered HTTP {status}: {reason}")]
    Http {
        url: String,
        status: u16,
        reason: String,
    },

    #[error("the request to {url} failed: {message}")]
    HttpTransport { url: String, message: String },

    #[error("downloaded file {path:?} does not match its published checksum (expected {algorithm} {expected}, computed {actual})")]
    ChecksumMismatch {
        path: PathBuf,
        algorithm: &'static str,
        expected: String,
        actual: String,
    },

    #[error("the plugin file {file:?} is already installed with different content; replace it explicitly to update")]
    PluginExists { file: String },

    #[error("the schedule is invalid: {reason}")]
    InvalidSchedule { reason: String },

    #[error("the schedule store is corrupt at {path:?}: {reason}")]
    SchedulesCorrupt { path: PathBuf, reason: String },

    #[error("extension {id:?} has an invalid manifest: {reason}")]
    InvalidExtensionManifest { id: String, reason: String },

    #[error("extension id {id:?} is invalid: ids are lowercase alphanumerics with dashes/underscores, starting with a letter or digit")]
    InvalidExtensionId { id: String },

    #[error("extension permission {permission:?} is not in the vocabulary; the model is deny-by-default")]
    InvalidExtensionPermission { permission: String },

    #[error("the extension folder {path:?} could not be read: {reason}")]
    ExtensionDirUnreadable { path: PathBuf, reason: String },

    #[error("the publish configuration is invalid: {reason}")]
    InvalidPublishConfig { reason: String },

    #[error("the publish state is corrupt at {path:?}: {reason}")]
    PublishStateCorrupt { path: PathBuf, reason: String },

    #[error(
        "the publish selection exceeds safety limits: {found} across {entries} entries, max {max_bytes} bytes / {max_entries} entries"
    )]
    PublishTooLarge {
        found: u64,
        entries: u64,
        max_bytes: u64,
        max_entries: u64,
    },

    #[error("the {provider} upload failed: {reason}")]
    PublishUpload { provider: String, reason: String },

    #[error("restore failed mid-commit ({reason}); the previous server files were rolled back — nothing was lost")]
    RestoreRolledBack { reason: String },

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

    /// The requested OS sandbox could not be built. This is a hard stop,
    /// never a silent downgrade: a server asked to run inside a boundary
    /// does not run outside it because the boundary failed to assemble.
    #[error("the sandbox could not be built: {detail}")]
    SandboxBuild { detail: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
