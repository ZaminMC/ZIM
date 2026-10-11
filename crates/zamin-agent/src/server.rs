//! The listening side: a TCP listener where every accepted connection is
//! TLS-wrapped and then relayed to the local daemon (ADR-0011). A failed
//! TLS handshake is logged and dropped — never a crash, never a reply.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::ServerConfig;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsStream};
use zamin_ipc::Endpoint;

use crate::relay::serve_connection;

/// Ceiling on concurrently live sessions. Each accepted connection holds
/// TLS buffers, a task, and — once authenticated — one local daemon
/// session; without a bound, a connection flood is an unbounded resource
/// commitment. Excess connections queue in the accept backlog instead.
const MAX_SESSIONS: usize = 64;

/// A breath between failed accepts: an fd-pressure condition needs a
/// moment to clear, not a hot spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(250);

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

    /// Accept forever. Per-connection errors are contained; transient
    /// accept errors (fd pressure, aborted connections) shed ONE accept,
    /// not the agent — the old `?` let a single EMFILE kill the process.
    /// The server itself has no failure mode short of the OS taking the
    /// socket away.
    pub async fn run(self) -> std::io::Result<()> {
        let endpoint = self.endpoint;
        let token = self.token;
        let sessions = Arc::new(tokio::sync::Semaphore::new(MAX_SESSIONS));
        loop {
            let (stream, peer) = match self.listener.accept().await {
                Ok(accepted) => accepted,
                Err(error) => {
                    if is_transient_accept_error(&error) {
                        tracing::warn!("accept failed ({error}); backing off");
                        tokio::time::sleep(ACCEPT_BACKOFF).await;
                        continue;
                    }
                    return Err(error);
                }
            };
            // Acquired BEFORE the spawn and held for the session's whole
            // lifetime: a full semaphore applies backpressure to the
            // accept loop itself instead of growing without bound.
            let permit = match sessions.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => continue, // never closed; unreachable, but honest
            };
            let acceptor = self.acceptor.clone();
            let endpoint = endpoint.clone();
            let token = token.clone();
            tokio::spawn(async move {
                let _permit = permit;
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

/// Accept errors that shed one connection rather than ending the
/// listener. ErrorKind covers the portable set; the fd-pressure
/// errnos (EMFILE/ENFILE/ENOMEM) have no ErrorKind arm, so that
/// question is asked through the platform seam (ADR-0008).
fn is_transient_accept_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::TimedOut
    ) || zamin_core::platform::is_fd_pressure_error(error)
}

/// A connected TLS stream, re-exported so tests and the binary stay
/// transport-honest about what `serve_connection` wraps.
pub type AcceptedTls = TlsStream<TcpStream>;
