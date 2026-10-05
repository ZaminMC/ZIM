//! Listening side: bind the endpoint (single-instance by construction) and
//! accept connections.

use crate::connection::Connection;
use crate::endpoint::Endpoint;
use crate::error::IpcError;
use crate::platform::PlatformServer;

pub struct IpcServer {
    endpoint: Endpoint,
    inner: PlatformServer,
}

impl IpcServer {
    /// Bind the endpoint. Binding fails with [`IpcError::AlreadyRunning`]
    /// when another daemon instance of the same user already owns it
    /// (ADR-0001: the bind itself is the single-instance mechanism).
    pub async fn bind(endpoint: Endpoint) -> Result<IpcServer, IpcError> {
        let inner = PlatformServer::bind(endpoint.clone()).await?;
        Ok(IpcServer { endpoint, inner })
    }

    /// Accept the next connection. The next listener instance is prepared
    /// before returning, keeping the busy window between clients minimal.
    pub async fn accept(&mut self) -> Result<Connection, IpcError> {
        self.inner.accept().await
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}
