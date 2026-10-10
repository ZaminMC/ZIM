//! Per-connection relay: gate the first frame, then move frames between
//! the remote TLS connection and one local daemon connection. One remote
//! connection maps to one daemon session — the daemon's per-session model
//! is preserved; the agent adds nothing to the conversation it relays.

use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncWrite};
use zamin_ipc::{Connection, ConnectionReadHalf, ConnectionWriteHalf, Endpoint, IpcError};
use zamin_protocol::envelope::{RequestId, Response};
use zamin_protocol::error::{ErrorCode, ProtocolError};

use crate::auth::{self, Gate};
use crate::AgentError;

pub async fn serve_connection(
    io: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    endpoint: Endpoint,
    token: &str,
) -> Result<(), AgentError> {
    let mut remote = Connection::new(io);
    // The gate runs before the local connection exists: an unauthenticated
    // peer never causes a daemon session. The wait for the first frame is
    // BOUNDED — an unauthenticated peer that connects and sends nothing
    // used to park its task and TLS buffers forever (a trivial remote
    // resource drain, one connection at a time).
    const FIRST_FRAME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
    let first = match tokio::time::timeout(FIRST_FRAME_TIMEOUT, remote.recv()).await {
        Ok(received) => received?,
        Err(_) => {
            tracing::debug!("closing a connection that never authenticated");
            return Ok(());
        }
    };
    let Some(first) = first else {
        return Ok(());
    };
    match auth::gate(&first, token) {
        Gate::Reject(response) => {
            tracing::warn!(
                "rejected remote connection: {}",
                response
                    .error
                    .as_ref()
                    .map(|e| e.code.as_str())
                    .unwrap_or("unknown")
            );
            let payload = serde_json::to_vec(&response)?;
            remote.send(payload.into()).await?;
            Ok(())
        }
        Gate::Forward { request_id } => {
            let mut local = match connect_local(endpoint).await {
                Ok(connection) => connection,
                Err(e) => {
                    return reply_unreachable(&mut remote, request_id, e).await;
                }
            };
            local.send(first).await?;
            pump(remote, local).await;
            Ok(())
        }
    }
}

/// The agent requires a running daemon and never spawns one: on a headless
/// host the daemon's service unit owns its lifetime (ADR-0011).
async fn connect_local(endpoint: Endpoint) -> Result<Connection, IpcError> {
    zamin_ipc::client::connect(endpoint).await
}

async fn reply_unreachable(
    remote: &mut Connection,
    request_id: RequestId,
    error: IpcError,
) -> Result<(), AgentError> {
    tracing::warn!("local daemon unreachable: {error}");
    let response = Response::err(
        request_id,
        ProtocolError::new(
            ErrorCode::DaemonUnreachable,
            format!("The agent cannot reach the local daemon: {error}."),
        )
        .with_remediation(&["start zamind on the agent's host"]),
    );
    let payload = serde_json::to_vec(&response)?;
    remote.send(payload.into()).await?;
    Ok(())
}

/// Frame relay in both directions. Either side ending the session closes
/// both connections — a half-open relay serves nobody.
async fn pump(remote: Connection, local: Connection) {
    let (remote_write, remote_read) = remote.split();
    let (local_write, local_read) = local.split();
    let up = direction(remote_read, local_write, "remote -> daemon");
    let down = direction(local_read, remote_write, "daemon -> remote");
    tokio::select! {
        _ = up => {},
        _ = down => {},
    }
}

async fn direction(
    mut read: ConnectionReadHalf,
    mut write: ConnectionWriteHalf,
    what: &'static str,
) {
    while let Ok(Some(frame)) = read.recv().await {
        let frame: Bytes = frame;
        if write.send(frame).await.is_err() {
            break;
        }
    }
    tracing::debug!("{what} stream closed");
}
