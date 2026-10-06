//! The `zamin` client core: one connection to the daemon, the mandatory
//! handshake, and request/reply demultiplexing over a single multiplexed
//! connection (protocol spec §1, §2).
//!
//! This is a *second client* implementation: it knows `zamin-protocol` and
//! `zamin-ipc` and nothing about the daemon's internals (ADR-0002). The
//! reader task is the single owner of the read half; replies route to the
//! waiting request by id, notifications fan out through a broadcast queue.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::{broadcast, mpsc, oneshot};
use zamin_ipc::{Connection, ConnectionReadHalf, ConnectionWriteHalf, IpcError};
use zamin_protocol::envelope::{IncomingMessage, Request, RequestId, Response};
use zamin_protocol::error::ProtocolError;
use zamin_protocol::handshake::{ClientInfo, HelloParams};
use zamin_protocol::methods;
use zamin_protocol::streams::{StreamKind, StreamNotification, SubscribeParams, SubscribeResult};

/// Client identity reported in the handshake.
pub const CLIENT_NAME: &str = "zamin-cli";
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Ipc(#[from] IpcError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("timed out after {0:?} waiting for the daemon's reply")]
    Timeout(Duration),
    #[error("the connection to the daemon closed before a reply arrived")]
    Disconnected,
    #[error("the daemon's reply was unreadable: {0}")]
    Malformed(String),
}

struct Shared {
    pending: Mutex<HashMap<u64, oneshot::Sender<Response>>>,
    notifications: broadcast::Sender<StreamNotification>,
}

/// A live connection to `zamind` past its handshake. Cloneable views are
/// not needed: the CLI drives one command at a time, and stream consumers
/// use the receivers returned by [`Client::subscribe`].
pub struct Client {
    shared: Arc<Shared>,
    writer: mpsc::Sender<Bytes>,
    next_id: AtomicU64,
}

impl Client {
    /// Connect and perform the mandatory `daemon.hello` exchange.
    pub async fn connect(endpoint: zamin_ipc::Endpoint) -> Result<Client, ClientError> {
        let connection: Connection = zamin_ipc::connect(endpoint).await?;
        let (write_half, read_half) = connection.split();
        Client::start(write_half, read_half).await
    }

    async fn start(
        write_half: ConnectionWriteHalf,
        read_half: ConnectionReadHalf,
    ) -> Result<Client, ClientError> {
        let (writer, mut writer_rx) = mpsc::channel::<Bytes>(64);
        tokio::spawn(async move {
            let mut write_half = write_half;
            while let Some(payload) = writer_rx.recv().await {
                if write_half.send(payload).await.is_err() {
                    break;
                }
            }
        });

        let (notifications, _) = broadcast::channel(256);
        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            notifications,
        });

        let reader_shared = Arc::clone(&shared);
        tokio::spawn(async move {
            let mut read_half = read_half;
            reader_loop(&reader_shared, &mut read_half).await;
        });

        let mut client = Client {
            shared,
            writer,
            next_id: AtomicU64::new(0),
        };
        client.hello().await?;
        Ok(client)
    }

    /// The mandatory first exchange (protocol spec §2). The daemon rejects
    /// version mismatches with a typed error, which surfaces unchanged.
    async fn hello(&mut self) -> Result<(), ClientError> {
        let params = HelloParams {
            protocol: zamin_protocol::PROTOCOL_VERSION,
            auth: None,
            client: ClientInfo {
                name: CLIENT_NAME.to_owned(),
                version: CLIENT_VERSION.to_owned(),
            },
        };
        let value = serde_json::to_value(&params)
            .map_err(|e| ClientError::Malformed(format!("hello params: {e}")))?;
        self.request_raw(methods::DAEMON_HELLO, value).await?;
        Ok(())
    }

    /// Send a request and await its reply. Stream notifications that
    /// arrive meanwhile are queued for subscribers, never lost.
    pub async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        self.request_raw(method, params).await
    }

    async fn request_raw(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let request = Request::new(RequestId::Number(id), method.to_owned(), Some(params));
        let payload = serde_json::to_vec(&request)
            .map_err(|e| ClientError::Malformed(format!("request {method}: {e}")))?;

        let (reply_tx, reply_rx) = oneshot::channel();
        self.shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, reply_tx);

        if self.writer.send(Bytes::from(payload)).await.is_err() {
            self.shared
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id);
            return Err(ClientError::Disconnected);
        }

        let response = match reply_rx.await {
            Ok(response) => response,
            Err(_sender_dropped) => return Err(ClientError::Disconnected),
        };

        match response {
            Response {
                result: Some(result),
                ..
            } => Ok(result),
            Response {
                error: Some(error), ..
            } => Err(ClientError::Protocol(error)),
            Response { .. } => Err(ClientError::Malformed(
                "response carries neither result nor error".to_owned(),
            )),
        }
    }

    /// Subscribe to a stream. The broadcast receiver is created *before*
    /// the subscribe request goes out, so the opening replay batch can
    /// never fall into the gap between reply and listener.
    pub async fn subscribe(
        &self,
        stream: StreamKind,
        server_id: Option<String>,
    ) -> Result<broadcast::Receiver<StreamNotification>, ClientError> {
        let receiver = self.shared.notifications.subscribe();
        let params = SubscribeParams {
            stream,
            server_id,
            cursor: None,
        };
        let value = serde_json::to_value(&params)
            .map_err(|e| ClientError::Malformed(format!("subscribe params: {e}")))?;
        let _result: SubscribeResult = self
            .request_typed(methods::STREAMS_SUBSCRIBE, value)
            .await?;
        Ok(receiver)
    }

    /// Typed request helper for params/results already modeled in
    /// `zamin-protocol`.
    pub async fn request_typed<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, ClientError> {
        let value = serde_json::to_value(params)
            .map_err(|e| ClientError::Malformed(format!("{method} params: {e}")))?;
        let result = self.request_raw(method, value).await?;
        serde_json::from_value(result)
            .map_err(|e| ClientError::Malformed(format!("{method} result: {e}")))
    }
}

/// The single reader: replies go to the pending request, notifications to
/// the broadcast queue. The loop ends when the daemon closes the wire.
async fn reader_loop(shared: &Shared, read_half: &mut ConnectionReadHalf) {
    loop {
        let frame = match read_half.recv().await {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => break,
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&frame) else {
            continue;
        };
        match IncomingMessage::parse(&value) {
            Some(IncomingMessage::Response(response)) => {
                let key = match &response.id {
                    RequestId::Number(n) => *n,
                    RequestId::String(s) => {
                        // A non-numeric string id cannot match any request
                        // this client sent (it numbers its own requests).
                        s.parse().unwrap_or(0)
                    }
                    RequestId::Null => 0,
                };
                if key == 0 {
                    continue;
                }
                if let Some(tx) = shared
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&key)
                {
                    let _ = tx.send(response);
                }
            }
            Some(IncomingMessage::Notification(note)) => {
                if let Ok(params) = note.parse_params::<StreamNotification>() {
                    // No receivers yet is fine: a fresh subscription is
                    // still resolving; its replay starts from now.
                    let _ = shared.notifications.send(params);
                }
            }
            Some(IncomingMessage::Request(_)) | None => {}
        }
    }
}
