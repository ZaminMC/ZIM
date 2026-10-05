//! Unix domain socket transport (ADR-0008 seam, zamin-ipc side).
//!
//! Single-instance and stale-socket handling: if the socket file exists, a
//! successful connect probe proves another daemon owns it; a failed probe
//! means the file is stale and is removed before binding.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tokio::net::{UnixListener, UnixStream};

use crate::connection::Connection;
use crate::endpoint::Endpoint;
use crate::error::IpcError;

pub struct PlatformServer {
    path: PathBuf,
    listener: UnixListener,
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
            std::fs::create_dir_all(parent)?;
            std::fs::set_permissions(parent, Permissions::from_mode(0o700))?;
        }

        if path.exists() {
            if UnixStream::connect(&path).await.is_ok() {
                return Err(IpcError::AlreadyRunning);
            }
            // Stale socket from a dead daemon: safe to reclaim.
            std::fs::remove_file(&path)?;
        }

        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, Permissions::from_mode(0o600))?;

        Ok(PlatformServer { path, listener })
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
