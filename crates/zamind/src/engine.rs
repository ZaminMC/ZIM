//! The engine: registry + actors + hub. This is the daemon's whole model
//! layer; sessions are thin over it and the protocol is the only way in.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};
use zamin_core::server::marker;
use zamin_core::server::registry::Registry;
use zamin_core::server::ServerId;
use zamin_protocol::error::{ErrorCode, ProtocolError};
use zamin_protocol::server::{LifecycleResult, ServerDetails, ServerState, ServerSummary};
use zamin_protocol::streams::{EventsSnapshot, StreamCursor, StreamKind, SubscribeResult};

use crate::actor::{Actor, ActorCommand, AdoptRecord};
use crate::hub::{HubError, HubHandle};

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct Inner {
    data_dir: PathBuf,
    registry: Mutex<Registry>,
    hub: HubHandle,
    actors: tokio::sync::Mutex<HashMap<String, mpsc::Sender<ActorCommand>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Hub(#[from] HubError),
    #[error("internal: {0}")]
    Internal(String),
}

fn server_dir(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join("servers").join(id)
}

impl Engine {
    pub async fn new(data_dir: PathBuf) -> Engine {
        let _ = std::fs::create_dir_all(data_dir.join("servers"));
        let registry = Registry::load(data_dir.join("registry.json")).unwrap_or_else(|e| {
            tracing::error!("registry is unreadable: {e}; refusing to start over it");
            std::process::exit(1);
        });
        Engine {
            inner: Arc::new(Inner {
                data_dir,
                registry: Mutex::new(registry),
                hub: HubHandle::new(),
                actors: tokio::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn hub(&self) -> &HubHandle {
        &self.inner.hub
    }

    fn registry_lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.inner
            .registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The per-server actor channel, creating the actor lazily in
    /// `not-running` if this daemon has never supervised it.
    async fn actor_for(
        &self,
        server_id: &ServerId,
    ) -> Result<mpsc::Sender<ActorCommand>, EngineError> {
        let mut actors = self.inner.actors.lock().await;
        if let Some(tx) = actors.get(server_id.as_str()) {
            return Ok(tx.clone());
        }
        let entry = {
            let registry = self.registry_lock();
            registry.get(server_id).map(|e| {
                (
                    e.root.clone(),
                    server_dir(&self.inner.data_dir, server_id.as_str()),
                )
            })
        };
        let Some((root, runtime_dir)) = entry else {
            return Err(not_found(server_id).into());
        };
        let _ = std::fs::create_dir_all(&runtime_dir);
        let tx = Actor::spawn_task(
            server_id.to_string(),
            root,
            runtime_dir,
            self.inner.data_dir.join("config.toml"),
            self.inner.hub.clone(),
            zamin_core::supervisor::state::ServerState::NotRunning,
        );
        actors.insert(server_id.to_string(), tx.clone());
        Ok(tx)
    }

    /// Adoption pass (ADR-0005): recorded runtime state + verified identity
    /// → running; identity mismatch → surfaced as unknown, never killed.
    pub async fn adopt_existing_servers(&self) {
        let entries: Vec<(ServerId, PathBuf, PathBuf)> = {
            let registry = self.registry_lock();
            registry
                .all()
                .map(|e| {
                    (
                        e.server_id.clone(),
                        e.root.clone(),
                        server_dir(&self.inner.data_dir, e.server_id.as_str()),
                    )
                })
                .collect()
        };
        for (server_id, root, runtime_dir) in entries {
            let record_path = runtime_dir.join("runtime.json");
            let Ok(bytes) = std::fs::read(&record_path) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                tracing::warn!(server = %server_id, "runtime record unreadable; skipping adoption");
                continue;
            };
            let (Some(pid), Some(start_marker)) = (
                value["pid"].as_u64().map(|p| p as u32),
                value["startMarker"].as_str().map(str::to_owned),
            ) else {
                continue;
            };
            let started_at_ms = value["startedAtMs"].as_i64().unwrap_or(0);
            let _ = std::fs::create_dir_all(&runtime_dir);
            let actor = Actor::new(
                server_id.to_string(),
                root,
                runtime_dir,
                self.inner.data_dir.join("config.toml"),
                self.inner.hub.clone(),
                zamin_core::supervisor::state::ServerState::Adopting,
            )
            .adopt(AdoptRecord {
                pid,
                start_marker,
                started_at_ms,
            });
            self.inner
                .actors
                .lock()
                .await
                .insert(server_id.to_string(), actor);
        }
    }

    pub async fn list_servers(&self) -> Vec<ServerSummary> {
        let entries: Vec<(ServerId, String)> = {
            let registry = self.registry_lock();
            registry
                .all()
                .map(|e| (e.server_id.clone(), e.display_name.clone()))
                .collect()
        };
        let mut out = Vec::new();
        for (server_id, display_name) in entries {
            let state = self.describe_state(&server_id).await;
            out.push(ServerSummary {
                server_id: server_id.to_string(),
                display_name,
                state,
            });
        }
        out
    }

    async fn describe_state(&self, server_id: &ServerId) -> ServerState {
        let actors = self.inner.actors.lock().await;
        match actors.get(server_id.as_str()) {
            Some(tx) => {
                let (reply, rx) = oneshot::channel();
                if tx
                    .clone()
                    .send(ActorCommand::Describe { reply })
                    .await
                    .is_ok()
                {
                    if let Ok(snapshot) = rx.await {
                        return snapshot.state;
                    }
                }
                ServerState::Unknown
            }
            None => ServerState::NotRunning,
        }
    }

    pub async fn get_server(&self, server_id: &ServerId) -> Result<ServerDetails, EngineError> {
        let display_name = self
            .registry_lock()
            .get(server_id)
            .map(|e| e.display_name.clone())
            .ok_or_else(|| not_found(server_id))?;
        Ok(ServerDetails {
            server_id: server_id.to_string(),
            display_name,
            state: self.describe_state(server_id).await,
            software: None,
            version: None,
            port: None,
        })
    }

    pub async fn register_server(
        &self,
        server_id: ServerId,
        display_name: String,
        root: PathBuf,
    ) -> Result<ServerDetails, EngineError> {
        if !root.is_dir() {
            return Err(ProtocolError::new(
                ErrorCode::FsNotFound,
                format!("The server directory {root:?} does not exist."),
            )
            .into());
        }
        let (details_root, display) = {
            let mut registry = self.registry_lock();
            let entry = registry
                .register(server_id.clone(), display_name.clone(), root)
                .map_err(|e| to_protocol(&e))?;
            (entry.root.clone(), entry.display_name.clone())
        };
        marker::write_marker(&details_root, &server_id).map_err(|e| to_protocol(&e))?;
        let _ = std::fs::create_dir_all(server_dir(&self.inner.data_dir, server_id.as_str()));
        Ok(ServerDetails {
            server_id: server_id.to_string(),
            display_name: display,
            state: ServerState::NotRunning,
            software: None,
            version: None,
            port: None,
        })
    }

    pub async fn rename_server(
        &self,
        server_id: &ServerId,
        display_name: String,
    ) -> Result<ServerDetails, EngineError> {
        self.registry_lock()
            .rename(server_id, display_name.clone())
            .map_err(|e| to_protocol(&e))?;
        self.get_server(server_id).await
    }

    pub async fn remove_server(&self, server_id: &ServerId) -> Result<(), EngineError> {
        let actor = self
            .inner
            .actors
            .lock()
            .await
            .get(server_id.as_str())
            .cloned();
        if let Some(tx) = actor {
            let (reply, rx) = oneshot::channel();
            tx.send(ActorCommand::Retire { reply })
                .await
                .map_err(|_| internal("actor is gone"))?;
            rx.await
                .map_err(|_| internal("actor dropped the reply"))??;
        }
        self.registry_lock()
            .remove(server_id)
            .map_err(|e| to_protocol(&e))?;
        let _ = std::fs::remove_dir_all(server_dir(&self.inner.data_dir, server_id.as_str()));
        Ok(())
    }

    pub async fn lifecycle(
        &self,
        server_id: &ServerId,
        command: LifecycleKind,
    ) -> Result<LifecycleResult, EngineError> {
        let tx = self.actor_for(server_id).await?;
        let (reply, rx) = oneshot::channel();
        let command = match command {
            LifecycleKind::Start => ActorCommand::Start { reply },
            LifecycleKind::Stop => ActorCommand::Stop { reply },
            LifecycleKind::Restart => ActorCommand::Restart { reply },
            LifecycleKind::Kill => ActorCommand::Kill { reply },
        };
        tx.send(command)
            .await
            .map_err(|_| internal("actor is gone"))?;
        let state = rx
            .await
            .map_err(|_| internal("actor dropped the reply"))??;
        Ok(LifecycleResult {
            server_id: server_id.to_string(),
            state,
        })
    }

    pub async fn write_stdin(&self, server_id: &ServerId, line: String) -> Result<(), EngineError> {
        let tx = self.actor_for(server_id).await?;
        let (reply, rx) = oneshot::channel();
        tx.send(ActorCommand::WriteStdin { line, reply })
            .await
            .map_err(|_| internal("actor is gone"))?;
        rx.await
            .map_err(|_| internal("actor dropped the reply"))??;
        Ok(())
    }

    pub async fn subscribe(
        &self,
        stream: StreamKind,
        server_id: Option<String>,
        cursor: Option<StreamCursor>,
    ) -> Result<
        (
            zamin_protocol::streams::SubscribeResult,
            crate::hub::Subscription,
        ),
        EngineError,
    > {
        let snapshot = if stream == StreamKind::Events && cursor.is_none() {
            Some(EventsSnapshot {
                servers: self.list_servers().await,
            })
        } else {
            None
        };
        let subscription = self
            .inner
            .hub
            .subscribe(stream, server_id, cursor)
            .map_err(EngineError::from)?;
        let result = SubscribeResult {
            subscription_id: subscription.id.clone(),
            cursor: match stream {
                StreamKind::Events => Some(StreamCursor::Events {
                    seq: self.inner.hub.current_seq(),
                }),
                _ => None,
            },
            snapshot,
            cursor_invalid: false,
        };
        Ok((result, subscription))
    }

    /// `logs.range`: the tail of the server's own `logs/latest.log`,
    /// parsed into protocol log lines (protocol spec §5, ADR-0006's
    /// file-backed catch-up path). The file read is sync and runs in
    /// `spawn_blocking`, per the rooted-filesystem contract.
    pub async fn log_range(
        &self,
        server_id: &ServerId,
        max_lines: u32,
    ) -> Result<zamin_protocol::logs::LogRangeResult, EngineError> {
        let root = {
            let registry = self.registry_lock();
            registry
                .get(server_id)
                .map(|e| e.root.clone())
                .ok_or_else(|| not_found(server_id))?
        };
        let server_id = server_id.to_string();
        let result =
            tokio::task::spawn_blocking(move || tail_of_latest_log(&root, &server_id, max_lines))
                .await
                .map_err(|e| EngineError::Internal(format!("log read task failed: {e}")))?;
        result.map_err(EngineError::Protocol)
    }

    pub async fn daemon_status(&self) -> serde_json::Value {
        let servers = self.list_servers().await;
        serde_json::json!({
            "protocol": zamin_protocol::PROTOCOL_VERSION,
            "daemon": { "name": crate::DAEMON_NAME, "version": crate::DAEMON_VERSION },
            "servers": servers.len(),
            "running": servers.iter().filter(|s| s.state == ServerState::Running).count(),
        })
    }
}

pub enum LifecycleKind {
    Start,
    Stop,
    Restart,
    Kill,
}

fn not_found(server_id: &ServerId) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::ServerNotFound,
        format!("Server {server_id} is not registered."),
    )
}

fn internal(message: &str) -> EngineError {
    EngineError::Internal(message.to_owned())
}

pub fn to_protocol(error: &zamin_core::error::CoreError) -> ProtocolError {
    // The actor owns the canonical mapping; sessions reuse it so the two
    // never diverge.
    crate::actor::to_protocol_error(error)
}

/// Sync tail read of `logs/latest.log` under the server root, through the
/// rooted filesystem's containment checks. Runs inside `spawn_blocking`.
fn tail_of_latest_log(
    root: &Path,
    server_id: &str,
    max_lines: u32,
) -> Result<zamin_protocol::logs::LogRangeResult, ProtocolError> {
    use zamin_protocol::logs::{LogRangeResult, LOG_RANGE_MAX_LINES, LOG_RANGE_WINDOW_BYTES};

    const FILE: &str = "logs/latest.log";
    let max_lines = max_lines.clamp(1, LOG_RANGE_MAX_LINES) as usize;

    let fs = zamin_core::fsops::RootedFs::open(root).map_err(|e| to_protocol(&e))?;
    let path = fs.resolve(FILE).map_err(|e| to_protocol(&e))?;
    if !path.is_file() {
        return Err(ProtocolError::new(
            ErrorCode::FsNotFound,
            format!("Server {server_id} has no log file yet ({FILE} does not exist in its root)."),
        ));
    }

    let mut file = std::fs::File::open(&path)
        .map_err(|source| zamin_core::error::CoreError::Io {
            path: path.clone(),
            source,
        })
        .map_err(|e| to_protocol(&e))?;
    let len = file
        .metadata()
        .map_err(|source| zamin_core::error::CoreError::Io {
            path: path.clone(),
            source,
        })
        .map_err(|e| to_protocol(&e))?
        .len();

    if len == 0 {
        return Ok(LogRangeResult {
            file: FILE.to_owned(),
            lines: Vec::new(),
            older_available: false,
        });
    }

    // Scan at most the trailing window; a partial first line (started
    // before the window) is dropped and reported via older_available.
    let window = len.min(LOG_RANGE_WINDOW_BYTES);
    file.seek(SeekFrom::Start(len - window))
        .map_err(|source| zamin_core::error::CoreError::Io {
            path: path.clone(),
            source,
        })
        .map_err(|e| to_protocol(&e))?;
    let mut buf = Vec::with_capacity(window as usize);
    file.read_to_end(&mut buf)
        .map_err(|source| zamin_core::error::CoreError::Io {
            path: path.clone(),
            source,
        })
        .map_err(|e| to_protocol(&e))?;
    let text = String::from_utf8_lossy(&buf);

    let mut raw_lines: Vec<&str> = text.split('\n').collect();
    let mut older_available = window < len;
    if window < len {
        // The first element is the cut-off remainder of an older line.
        if !raw_lines.is_empty() {
            raw_lines.remove(0);
        }
    }
    // A trailing "" after the final newline is not a line.
    if raw_lines.last().is_some_and(|l| l.is_empty()) {
        raw_lines.pop();
    }

    if raw_lines.len() > max_lines {
        let drop = raw_lines.len() - max_lines;
        raw_lines.drain(..drop);
        older_available = true;
    }

    let lines = raw_lines
        .iter()
        .map(|raw| zamin_core::logparse::parse_line(raw, 0))
        .collect();

    Ok(LogRangeResult {
        file: FILE.to_owned(),
        lines,
        older_available,
    })
}
