//! Client side: connect to the daemon's endpoint.

use crate::connection::Connection;
use crate::endpoint::Endpoint;
use crate::error::IpcError;
use crate::platform;

/// Connect to the daemon. Short-lived ERROR_PIPE_BUSY windows on Windows are
/// retried briefly; anything longer means the daemon is gone.
pub async fn connect(endpoint: Endpoint) -> Result<Connection, IpcError> {
    platform::connect(endpoint).await
}
