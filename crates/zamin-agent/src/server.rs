//! The listening side: a TCP listener where every accepted connection is
//! TLS-wrapped and then relayed to the local daemon (ADR-0011). A failed
//! TLS handshake is logged and dropped — never a crash, never a reply.

use std::net::SocketAddr;
use std::sync::Arc;

use rustls::ServerConfig;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsStream};
use zamin_ipc::Endpoint;

use crate::relay::serve_connection;

pub struct AgentServer {
    listener: TcpListener,
    addr: SocketAddr,
    acceptor: TlsAcceptor,
    endpoint: Endpoint,
    token: String,
}

impl AgentServer {
    /// Bind the listen address. The single-instance mechanism is the
    /// daemon's, not the agent's — two agents on one address is an OS
    /// bind error, reported as such.
    pub async fn bind(
        listen: SocketAddr,
        tls: Arc<ServerConfig>,
        endpoint: Endpoint,
        token: String,
    ) -> Result<AgentServer, std::io::Error> {
        let listener = TcpListener::bind(listen).await?;
        let addr = listener.local_addr()?;
        Ok(AgentServer {
            listener,
            addr,
            acceptor: TlsAcceptor::from(tls),
            endpoint,
            token,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Accept forever. Per-connection errors are contained; the server
    /// itself has no failure mode short of the OS taking the socket away.
    pub async fn run(self) -> std::io::Result<()> {
        let endpoint = self.endpoint;
        let token = self.token;
        loop {
            let (stream, peer) = self.listener.accept().await?;
            let acceptor = self.acceptor.clone();
            let endpoint = endpoint.clone();
            let token = token.clone();
            tokio::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(tls) => {
                        if let Err(e) = serve_connection(tls, endpoint, &token).await {
                            tracing::warn!("relay ended for {peer}: {e}");
                        }
                    }
                    Err(e) => {
                        // Plaintext scanners and aborted handshakes land
                        // here; they get nothing but the drop.
                        tracing::debug!("tls handshake failed for {peer}: {e}");
                    }
                }
            });
        }
    }
}

/// A connected TLS stream, re-exported so tests and the binary stay
/// transport-honest about what `serve_connection` wraps.
pub type AcceptedTls = TlsStream<TcpStream>;
