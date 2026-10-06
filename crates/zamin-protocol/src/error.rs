//! Typed protocol errors: stable string codes, a human message, structured
//! context, and remediation action IDs (protocol spec §4).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable, machine-readable error codes. Registered here as the single
/// source of truth; ad-hoc codes are a review rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ProtocolVersionUnsupported,
    ProtocolInvalidRequest,
    ProtocolMethodNotFound,
    DaemonBusy,
    ServerNotFound,
    ServerIdExists,
    ServerIdInvalid,
    ServerAlreadyRunning,
    ServerNotRunning,
    ServerStartTimeout,
    PreflightFailed,
    NeedsEula,
    JavaNotFound,
    JavaIncompatible,
    JavaExecFailed,
    PortInUse,
    FsOutsideRoot,
    FsNotWritable,
    FsNotFound,
    FsPathEscapesRoot,
    LogCursorInvalid,
    ArchiveUnsafeEntry,
    DiskFull,
    JobNotFound,
    JobNotCancellable,
    CatalogUnavailable,
    CatalogNotFound,
    ChecksumMismatch,
    InternalError,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::ProtocolVersionUnsupported => "PROTOCOL_VERSION_UNSUPPORTED",
            ErrorCode::ProtocolInvalidRequest => "PROTOCOL_INVALID_REQUEST",
            ErrorCode::ProtocolMethodNotFound => "PROTOCOL_METHOD_NOT_FOUND",
            ErrorCode::DaemonBusy => "DAEMON_BUSY",
            ErrorCode::ServerNotFound => "SERVER_NOT_FOUND",
            ErrorCode::ServerIdExists => "SERVER_ID_EXISTS",
            ErrorCode::ServerIdInvalid => "SERVER_ID_INVALID",
            ErrorCode::ServerAlreadyRunning => "SERVER_ALREADY_RUNNING",
            ErrorCode::ServerNotRunning => "SERVER_NOT_RUNNING",
            ErrorCode::ServerStartTimeout => "SERVER_START_TIMEOUT",
            ErrorCode::PreflightFailed => "PREFLIGHT_FAILED",
            ErrorCode::NeedsEula => "NEEDS_EULA",
            ErrorCode::JavaNotFound => "JAVA_NOT_FOUND",
            ErrorCode::JavaIncompatible => "JAVA_INCOMPATIBLE",
            ErrorCode::JavaExecFailed => "JAVA_EXEC_FAILED",
            ErrorCode::PortInUse => "PORT_IN_USE",
            ErrorCode::FsOutsideRoot => "FS_OUTSIDE_ROOT",
            ErrorCode::FsNotWritable => "FS_NOT_WRITABLE",
            ErrorCode::FsNotFound => "FS_NOT_FOUND",
            ErrorCode::FsPathEscapesRoot => "FS_PATH_ESCAPES_ROOT",
            ErrorCode::LogCursorInvalid => "LOG_CURSOR_INVALID",
            ErrorCode::ArchiveUnsafeEntry => "ARCHIVE_UNSAFE_ENTRY",
            ErrorCode::DiskFull => "DISK_FULL",
            ErrorCode::JobNotFound => "JOB_NOT_FOUND",
            ErrorCode::JobNotCancellable => "JOB_NOT_CANCELLABLE",
            ErrorCode::CatalogUnavailable => "CATALOG_UNAVAILABLE",
            ErrorCode::CatalogNotFound => "CATALOG_NOT_FOUND",
            ErrorCode::ChecksumMismatch => "CHECKSUM_MISMATCH",
            ErrorCode::InternalError => "INTERNAL_ERROR",
        }
    }
}

/// The wire error object. `message` is a complete, specific sentence; see the
/// style guide's message rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remediation: Vec<String>,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ProtocolError {
            code,
            message: message.into(),
            context: BTreeMap::new(),
            remediation: Vec::new(),
        }
    }

    pub fn with_context(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.context.insert(key.to_owned(), value.into());
        self
    }

    pub fn with_remediation(mut self, actions: &[&str]) -> Self {
        self.remediation = actions.iter().map(|a| (*a).to_owned()).collect();
        self
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ProtocolError {}
