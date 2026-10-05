//! Windows named-pipe transport (ADR-0008 seam, zamin-ipc side).
//!
//! Single-instance: the first `first_pipe_instance` bind owns the name; a
//! second bind fails with ERROR_ACCESS_DENIED → `AlreadyRunning`.

use std::time::{Duration, Instant};

use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

use crate::connection::Connection;
use crate::endpoint::Endpoint;
use crate::error::IpcError;

const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PIPE_BUSY: i32 = 231;
const BUSY_RETRY_WINDOW: Duration = Duration::from_secs(5);

fn full_pipe_name(name: &str) -> String {
    format!(r"\\.\pipe\{name}")
}

pub struct PlatformServer {
    pipe_name: String,
    pending: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
}

impl PlatformServer {
    pub async fn bind(endpoint: Endpoint) -> Result<PlatformServer, IpcError> {
        let name = match endpoint {
            Endpoint::WindowsPipe(name) => name,
            other => {
                return Err(IpcError::EndpointInvalid(format!(
                    "{other:?} is not a Windows endpoint"
                )))
            }
        };
        let full = full_pipe_name(&name);
        let first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&full)
            .map_err(|e| {
                if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
                    IpcError::AlreadyRunning
                } else {
                    IpcError::from(e)
                }
            })?;
        Ok(PlatformServer {
            pipe_name: full,
            pending: Some(first),
        })
    }

    pub async fn accept(&mut self) -> Result<Connection, IpcError> {
        let server = self
            .pending
            .take()
            .ok_or_else(|| IpcError::EndpointInvalid("server is not bound".to_owned()))?;
        server.connect().await?;
        self.pending = Some(ServerOptions::new().create(&self.pipe_name)?);
        Ok(Connection::new(server))
    }
}

pub async fn connect(endpoint: Endpoint) -> Result<Connection, IpcError> {
    let name = match endpoint {
        Endpoint::WindowsPipe(name) => full_pipe_name(&name),
        other => {
            return Err(IpcError::EndpointInvalid(format!(
                "{other:?} is not a Windows endpoint"
            )))
        }
    };
    let deadline = Instant::now() + BUSY_RETRY_WINDOW;
    loop {
        match ClientOptions::new().open(&name) {
            Ok(client) => return Ok(Connection::new(client)),
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => {
                return Err(IpcError::NoDaemon)
            }
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                if Instant::now() >= deadline {
                    return Err(IpcError::NoDaemon);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}
