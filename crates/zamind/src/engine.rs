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
        // The events stream is the reconcile channel (ADR-0006): clients
        // with an open subscription must learn about new servers without
        // polling, so registration is broadcast as a transition.
        self.publish_registry_event(
            &server_id,
            ServerState::Unknown,
            ServerState::NotRunning,
            "registered",
        );
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
        // Mirror of the registration broadcast: open panels drop the entry.
        self.publish_registry_event(
            server_id,
            ServerState::NotRunning,
            ServerState::Unknown,
            "removed",
        );
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
        before_offset: Option<u64>,
    ) -> Result<zamin_protocol::logs::LogRangeResult, EngineError> {
        let root = {
            let registry = self.registry_lock();
            registry
                .get(server_id)
                .map(|e| e.root.clone())
                .ok_or_else(|| not_found(server_id))?
        };
        let server_id = server_id.to_string();
        let result = tokio::task::spawn_blocking(move || {
            range_of_latest_log(&root, &server_id, max_lines, before_offset)
        })
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

    /// Resolve a registered server's root for a file-manager call, or the
    /// typed SERVER_NOT_FOUND.
    fn file_root(&self, server_id: &ServerId) -> Result<PathBuf, EngineError> {
        let registry = self.registry_lock();
        registry
            .get(server_id)
            .map(|e| e.root.clone())
            .ok_or_else(|| not_found(server_id))
            .map_err(EngineError::Protocol)
    }

    /// Run one sync file-manager operation off the async runtime.
    async fn file_op<T>(
        &self,
        server_id: &ServerId,
        op: impl FnOnce(&Path) -> Result<T, ProtocolError> + Send + 'static,
    ) -> Result<T, EngineError>
    where
        T: Send + 'static,
    {
        let root = self.file_root(server_id)?;
        tokio::task::spawn_blocking(move || op(&root))
            .await
            .map_err(|e| EngineError::Internal(format!("file operation task failed: {e}")))?
            .map_err(EngineError::Protocol)
    }

    pub async fn files_list(
        &self,
        server_id: &ServerId,
        path: &str,
        offset: u32,
        limit: u32,
    ) -> Result<zamin_protocol::files::FilesListResult, EngineError> {
        let path = path.to_owned();
        self.file_op(server_id, move |root| {
            crate::files::list(root, &path, offset, limit)
        })
        .await
    }

    pub async fn files_read(
        &self,
        server_id: &ServerId,
        path: &str,
        offset: u64,
        max_bytes: u32,
    ) -> Result<zamin_protocol::files::FilesReadResult, EngineError> {
        let path = path.to_owned();
        self.file_op(server_id, move |root| {
            crate::files::read(root, &path, offset, max_bytes)
        })
        .await
    }

    pub async fn files_write(
        &self,
        server_id: &ServerId,
        staging_id: Option<String>,
        content: &str,
    ) -> Result<zamin_protocol::files::FilesWriteResult, EngineError> {
        let content = content.to_owned();
        self.file_op(server_id, move |root| {
            crate::files::write(root, staging_id.as_deref(), &content)
        })
        .await
    }

    pub async fn files_commit(
        &self,
        server_id: &ServerId,
        staging_id: &str,
        target: &str,
    ) -> Result<zamin_protocol::files::FilesCommitResult, EngineError> {
        let staging_id = staging_id.to_owned();
        let target = target.to_owned();
        self.file_op(server_id, move |root| {
            crate::files::commit(root, &staging_id, &target)
        })
        .await
    }

    pub async fn files_mkdir(&self, server_id: &ServerId, path: &str) -> Result<(), EngineError> {
        let path = path.to_owned();
        self.file_op(server_id, move |root| crate::files::mkdir(root, &path))
            .await
    }

    pub async fn files_rename(
        &self,
        server_id: &ServerId,
        from: &str,
        to: &str,
    ) -> Result<(), EngineError> {
        let from = from.to_owned();
        let to = to.to_owned();
        self.file_op(server_id, move |root| {
            crate::files::rename(root, &from, &to)
        })
        .await
    }

    pub async fn files_delete(&self, server_id: &ServerId, path: &str) -> Result<(), EngineError> {
        let path = path.to_owned();
        self.file_op(server_id, move |root| crate::files::delete(root, &path))
            .await
    }

    /// Broadcast a registry-driven transition (registration, removal).
    /// These are state changes from the registry's point of view, not the
    /// actor's, so they are published here rather than in an actor.
    fn publish_registry_event(
        &self,
        server_id: &ServerId,
        from: ServerState,
        to: ServerState,
        reason: &str,
    ) {
        self.inner.hub.publish_event(
            Some(server_id.to_string()),
            zamin_protocol::streams::CoreEvent::ServerStateChanged {
                server_id: server_id.to_string(),
                from,
                to,
                reason: Some(reason.to_owned()),
                exit_code: None,
                error: None,
                crash: None,
            },
        );
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

/// Sync ranged read of `logs/latest.log` under the server root, through
/// the rooted filesystem's containment checks. Runs inside `spawn_blocking`.
///
/// Returns the last `max_lines` lines that end at or before `cursor` (where
/// `None` means the end of the file), plus the byte offset where the first
/// returned line starts — the client's next `beforeOffset`.
///
/// The read walks the file backward in [`LOG_RANGE_WINDOW_BYTES`] windows,
/// accumulating bytes until the requested lines are complete, so one call
/// reads roughly the page plus one window — never the whole file, and a
/// tail never re-reads from offset 0 (PERFORMANCE-BUDGETS). A single
/// overlong line is the one unbounded case and is served whole.
fn range_of_latest_log(
    root: &Path,
    server_id: &str,
    max_lines: u32,
    before_offset: Option<u64>,
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

    // Resolve the cursor. A cursor past the current end of the file means
    // the file rotated or truncated under the client: its offsets no
    // longer address this content, and honoring them would serve garbled
    // pages — a typed error beats silent nonsense.
    let cursor = match before_offset {
        None => len,
        Some(offset) if offset <= len => offset,
        Some(offset) => {
            return Err(ProtocolError::new(
                ErrorCode::LogCursorInvalid,
                format!(
                    "beforeOffset {offset} is beyond the current end of {FILE} ({len} bytes); \
                     the file was rotated or truncated — page from the tail again."
                ),
            ));
        }
    };

    // The answer is empty without any byte range to walk: at the file
    // start there is nothing before the cursor.
    if cursor == 0 {
        return Ok(LogRangeResult {
            file: FILE.to_owned(),
            lines: Vec::new(),
            older_available: false,
            start_offset: 0,
        });
    }

    // Walk backward from the cursor, prepending windows, until the buffer
    // spans [window_start, cursor) with at least `max_lines` complete
    // lines inside — or the file start is reached. `buf` always ends at
    // the cursor, so its tail rules are fixed; only its head is partial
    // until the file start is covered.
    let mut buf: Vec<u8> = Vec::new();
    let mut window_start = cursor;
    while window_start > 0 {
        let window = window_start.min(LOG_RANGE_WINDOW_BYTES);
        window_start -= window;
        let mut chunk = vec![0u8; window as usize];
        file.seek(SeekFrom::Start(window_start))
            .map_err(|source| zamin_core::error::CoreError::Io {
                path: path.clone(),
                source,
            })
            .map_err(|e| to_protocol(&e))?;
        file.read_exact(&mut chunk)
            .map_err(|source| zamin_core::error::CoreError::Io {
                path: path.clone(),
                source,
            })
            .map_err(|e| to_protocol(&e))?;
        let mut merged = Vec::with_capacity(chunk.len() + buf.len());
        merged.extend_from_slice(&chunk);
        merged.extend_from_slice(&buf);
        buf = merged;
        if window_start == 0 || complete_lines_in(&buf, window_start, cursor, len) >= max_lines {
            break;
        }
    }

    // Parse the accumulated buffer once. `window_start` is where buf
    // begins in the file; the head rule (drop the partial first part
    // unless the buffer starts at the file start) and the tail rule (drop
    // the part past the cursor, or the "" artifact of a final newline)
    // together select exactly the complete lines inside [window_start,
    // cursor).
    let text = String::from_utf8_lossy(&buf);
    let parts: Vec<&str> = text.split('\n').collect();
    let head = usize::from(window_start > 0);
    let tail = if cursor < len || parts.last().is_some_and(|l| l.is_empty()) {
        1
    } else {
        0
    };
    let included_end = parts.len().saturating_sub(tail);
    let included = if head < included_end {
        &parts[head..included_end]
    } else {
        &[][..]
    };

    // Serve the LAST max_lines of the included lines; anything above them
    // (or an unread window head) is what `olderAvailable` offers.
    let served = included.len().min(max_lines);
    let first_served = included.len() - served;

    // Byte offset of the first served line: sum the lengths of every part
    // before it, counting each part's delimiter.
    let start_offset = window_start
        + parts[..head + first_served]
            .iter()
            .map(|part| part.len() as u64 + 1)
            .sum::<u64>();

    let lines = included[first_served..]
        .iter()
        .map(|raw| zamin_core::logparse::parse_line(raw, 0))
        .collect();

    Ok(LogRangeResult {
        file: FILE.to_owned(),
        lines,
        older_available: start_offset > 0,
        start_offset,
    })
}

/// Count the complete lines inside `buf`, which spans `[buf_start, cursor)`
/// of the file. The head part is complete only at the file start; the tail
/// part is complete only when the buffer ends at the file's end with a
/// final line (not a newline artifact).
fn complete_lines_in(buf: &[u8], buf_start: u64, cursor: u64, len: u64) -> usize {
    let text = String::from_utf8_lossy(buf);
    let parts = text.split('\n').count();
    let head = usize::from(buf_start > 0);
    let tail = if cursor < len || text.ends_with('\n') {
        1
    } else {
        0
    };
    parts.saturating_sub(head + tail)
}
