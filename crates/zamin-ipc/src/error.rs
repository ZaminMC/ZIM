//! Transport-level errors. Domain errors (servers, jobs, ports) never
//! originate here — this crate speaks frames, not semantics.

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("another daemon instance is already running for this user")]
    AlreadyRunning,
    #[error("daemon is not running")]
    NoDaemon,
    #[error("daemon is alive but out of connection slots")]
    DaemonBusy,
    #[error("ipc endpoint is not usable here: {0}")]
    EndpointInvalid(String),
    #[error("frame exceeds the protocol limit")]
    FrameTooLarge,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
