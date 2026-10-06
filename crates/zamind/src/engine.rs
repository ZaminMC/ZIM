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
use crate::jobs::{JobFailure, JobRunner};

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct Inner {
    data_dir: PathBuf,
    registry: Mutex<Registry>,
    hub: HubHandle,
    actors: tokio::sync::Mutex<HashMap<String, mpsc::Sender<ActorCommand>>>,
    jobs: JobRunner,
    /// The software catalog's base URL (Fill API v3); overridable so
    /// tests and air-gapped installs can point at a mirror.
    catalog_url: String,
}

/// After `save-all`, a server needs a moment to actually finish writing
/// region files before the archive walk snapshots them. Fixed and small;
/// the save commands themselves are awaited through the actor's stdin
/// ack, this settle covers the server's own flush completion.
const LIVE_SAVE_SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

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
    pub async fn with_catalog_url(data_dir: PathBuf, catalog_url: String) -> Engine {
        let _ = std::fs::create_dir_all(data_dir.join("servers"));
        let registry = Registry::load(data_dir.join("registry.json")).unwrap_or_else(|e| {
            tracing::error!("registry is unreadable: {e}; refusing to start over it");
            std::process::exit(1);
        });
        let hub = HubHandle::new();
        let jobs = JobRunner::new(hub.clone());
        Engine {
            inner: Arc::new(Inner {
                data_dir,
                registry: Mutex::new(registry),
                hub,
                actors: tokio::sync::Mutex::new(HashMap::new()),
                jobs,
                catalog_url,
            }),
        }
    }

    pub fn hub(&self) -> &HubHandle {
        &self.inner.hub
    }

    /// The daemon-wide job runner. Events for jobs ride the regular
    /// events stream; the runner holds the queryable records.
    pub fn jobs(&self) -> &JobRunner {
        &self.inner.jobs
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

    /// `players.list`: a Server List Ping against the server's configured
    /// port. A server that is off, or has no port configured, answers
    /// with an honest "nobody" shape — that is a normal state, not an
    /// error (the Players page renders it as an empty room).
    pub async fn players_list(
        &self,
        server_id: &ServerId,
    ) -> Result<zamin_protocol::players::PlayersListResult, EngineError> {
        // Validates registration before any ping attempt.
        self.file_root(server_id)?;
        let data_dir = self.inner.data_dir.clone();
        let id = server_id.to_string();
        let port = tokio::task::spawn_blocking(move || {
            let global =
                zamin_core::config::load_global(&data_dir.join("config.toml")).unwrap_or_default();
            let per = zamin_core::config::load_server(
                &data_dir.join("servers").join(&id).join("config.toml"),
            )
            .unwrap_or_default();
            zamin_core::config::layer(&global.defaults, &per.settings).port
        })
        .await
        .map_err(|e| EngineError::Internal(format!("settings task failed: {e}")))?;
        let Some(port) = port else {
            return Err(EngineError::Protocol(ProtocolError::new(
                ErrorCode::ProtocolInvalidRequest,
                format!(
                    "Server {server_id} has no port configured — the players surface needs \
                     the server's port to ping it."
                ),
            )));
        };

        let started = tokio::time::Instant::now();
        let addr = format!("127.0.0.1:{port}");
        let ping = zamin_core::ping::server_list_ping(&addr, "127.0.0.1", port).await;
        let latency = started.elapsed().as_millis() as u32;
        match ping {
            Ok(status) => {
                let motd = status.motd();
                Ok(zamin_protocol::players::PlayersListResult {
                    source: zamin_protocol::players::PlayersSource::Ping,
                    online: status.players.online,
                    max: status.players.max,
                    sample: status
                        .players
                        .sample
                        .unwrap_or_default()
                        .into_iter()
                        .map(|entry| zamin_protocol::players::PlayerSample {
                            name: entry.name,
                            id: entry.id,
                        })
                        .collect(),
                    latency_ms: latency,
                    version: status.version.as_ref().and_then(|v| v.name.clone()),
                    motd,
                })
            }
            // Unreachable is a shape, not an error: the room is simply
            // empty (server off, starting, or lying about its port).
            // Latency still reports the attempt.
            Err(_) => Ok(zamin_protocol::players::PlayersListResult {
                source: zamin_protocol::players::PlayersSource::Ping,
                online: None,
                max: None,
                sample: Vec::new(),
                latency_ms: latency,
                version: None,
                motd: None,
            }),
        }
    }

    // --- backups (protocol spec §7 jobs; the ADR-0009 safety model) -----

    /// Backups live under the daemon's data dir, one folder per server:
    /// `<data>/backups/<serverId>/<backupId>.tar.gz` (+ `.json` manifests).
    fn backups_dir(&self, server_id: &str) -> PathBuf {
        self.inner.data_dir.join("backups").join(server_id)
    }

    /// The server's backups, newest first, from the manifests on disk.
    pub async fn backups_list(
        &self,
        server_id: &ServerId,
    ) -> Result<zamin_protocol::backups::BackupsListResult, EngineError> {
        self.file_root(server_id)?;
        let dir = self.backups_dir(server_id.as_str());
        let manifests = tokio::task::spawn_blocking(move || zamin_core::backup::list_backups(&dir))
            .await
            .map_err(|e| EngineError::Internal(format!("backups list task failed: {e}")))?;
        Ok(zamin_protocol::backups::BackupsListResult {
            backups: manifests
                .into_iter()
                .rev()
                .map(|m| zamin_protocol::backups::BackupInfo {
                    backup_id: m.backup_id,
                    created_at_ms: m.created_at_ms,
                    size_bytes: m.size_bytes,
                    total_bytes: m.total_bytes,
                    file_count: m.file_count,
                    label: m.label,
                    taken: m.taken,
                })
                .collect(),
        })
    }

    /// `backup.create`: a job. For a running server the archive walk is
    /// wrapped in a save window (`save-off` → `save-all` → settle → walk
    /// → `save-on`, ADR-0009); otherwise it is a cold copy. Retention
    /// prunes older backups after a successful create.
    pub async fn backup_create(
        &self,
        server_id: &ServerId,
        label: Option<String>,
    ) -> Result<zamin_protocol::jobs::Job, EngineError> {
        let root = self.file_root(server_id)?;
        let live = self.describe_state(server_id).await == ServerState::Running;

        let data_dir = self.inner.data_dir.clone();
        let sid = server_id.to_string();
        let keep = tokio::task::spawn_blocking(move || {
            let global =
                zamin_core::config::load_global(&data_dir.join("config.toml")).unwrap_or_default();
            let per = zamin_core::config::load_server(
                &data_dir.join("servers").join(&sid).join("config.toml"),
            )
            .unwrap_or_default();
            zamin_core::config::layer(&global.defaults, &per.settings).backup_keep
        })
        .await
        .map_err(|e| EngineError::Internal(format!("settings task failed: {e}")))?;
        let keep = keep.max(1) as usize;

        let engine_for_save = self.clone();
        let sid_for_save = server_id.clone();
        let sid_for_archive = server_id.to_string();
        let backups_dir = self.backups_dir(server_id.as_str());
        let taken = if live {
            zamin_protocol::jobs::BackupTaken::Live
        } else {
            zamin_protocol::jobs::BackupTaken::Cold
        };

        Ok(self.inner.jobs.spawn(
            zamin_protocol::jobs::JobKind::BackupCreate,
            Some(server_id.to_string()),
            move |ctl| async move {
                if live {
                    ctl.progress(
                        0,
                        None,
                        None,
                        Some("asking the server to flush (save-off / save-all)"),
                    );
                    engine_for_save
                        .write_stdin(&sid_for_save, "save-off".to_owned())
                        .await
                        .map_err(job_failure)?;
                    engine_for_save
                        .write_stdin(&sid_for_save, "save-all".to_owned())
                        .await
                        .map_err(job_failure)?;
                    tokio::time::sleep(LIVE_SAVE_SETTLE).await;
                }
                if ctl.cancelled() {
                    if live {
                        let _ = engine_for_save
                            .write_stdin(&sid_for_save, "save-on".to_owned())
                            .await;
                    }
                    return Err(JobFailure::Cancelled);
                }

                let progress_ctl = ctl.clone();
                let archive_ctl = ctl.clone();
                let prune_dir = backups_dir.clone();
                let walk = tokio::task::spawn_blocking(move || {
                    zamin_core::backup::create_archive(
                        &root,
                        &backups_dir,
                        zamin_core::backup::BackupCreateOptions {
                            server_id: sid_for_archive,
                            label,
                            taken,
                            cancel: archive_ctl.cancel_flag(),
                            progress: Arc::new(move |p: zamin_core::backup::CreateProgress| {
                                progress_ctl.progress(
                                    p.bytes_done,
                                    Some(p.bytes_done.max(1)),
                                    Some("bytes"),
                                    None,
                                );
                            }),
                        },
                    )
                })
                .await
                .map_err(|e| {
                    JobFailure::Error(ProtocolError::new(
                        ErrorCode::InternalError,
                        format!("backup task failed: {e}"),
                    ))
                })?;

                let result = match walk {
                    Ok(outcome) => {
                        ctl.progress(
                            outcome.manifest.size_bytes,
                            Some(outcome.manifest.size_bytes.max(1)),
                            Some("bytes"),
                            Some(&format!(
                                "backup {} written ({} files)",
                                outcome.backup_id, outcome.manifest.file_count
                            )),
                        );
                        // Retention (ARCH-REVIEW §16.5): janitorial, never fatal.
                        if let Err(e) = zamin_core::backup::prune_backups(&prune_dir, keep) {
                            tracing::warn!("retention pruning failed: {e}");
                        }
                        Ok(())
                    }
                    Err(zamin_core::error::CoreError::Cancelled) => Err(JobFailure::Cancelled),
                    Err(e) => Err(JobFailure::Error(crate::engine::to_protocol(&e))),
                };

                if live {
                    let _ = engine_for_save
                        .write_stdin(&sid_for_save, "save-on".to_owned())
                        .await;
                }
                result
            },
        ))
    }

    /// `backup.restore`: a job that replaces the server root with the
    /// backup's content. Refused while the server is not `not-running` —
    /// files a running server holds open cannot be replaced (Windows
    /// locks them; Linux would restore under a live world).
    pub async fn backup_restore(
        &self,
        server_id: &ServerId,
        backup_id: uuid::Uuid,
    ) -> Result<zamin_protocol::jobs::Job, EngineError> {
        let root = self.file_root(server_id)?;
        let archive =
            zamin_core::backup::archive_path(&self.backups_dir(server_id.as_str()), backup_id);
        if !archive.is_file() {
            return Err(ProtocolError::new(
                ErrorCode::FsNotFound,
                format!(
                    "Backup {backup_id} does not exist for server {server_id}; list the backups first."
                ),
            )
            .into());
        }
        let state = self.describe_state(server_id).await;
        // Everything that holds no open server files is restorable. There
        // is more than one "stopped" state (ADR-0005): `not-running` (never
        // started / reset) and `stopped` (a graceful stop ended it) — plus
        // the failure states where the process is gone. Only live-ish
        // states refuse; the smoke test caught exactly this: a graceful
        // stop lands on `stopped`, not `not-running`.
        match state {
            ServerState::NotRunning
            | ServerState::Stopped
            | ServerState::FailedPreflight
            | ServerState::Crashed => {}
            other => {
                return Err(ProtocolError::new(
                    ErrorCode::ServerAlreadyRunning,
                    format!(
                        "Server {server_id} is {other:?}; stop it before restoring a backup — open files cannot be replaced."
                    ),
                )
                .into());
            }
        }

        Ok(self.inner.jobs.spawn(
            zamin_protocol::jobs::JobKind::BackupRestore,
            Some(server_id.to_string()),
            move |ctl| async move {
                if ctl.cancelled() {
                    return Err(JobFailure::Cancelled);
                }
                let progress_ctl = ctl.clone();
                let extract_ctl = ctl.clone();
                let restore = tokio::task::spawn_blocking(move || {
                    zamin_core::backup::restore_archive(
                        &root,
                        &archive,
                        &zamin_core::backup::RestoreOptions {
                            cancel: extract_ctl.cancel_flag(),
                            progress: Arc::new(move |p: zamin_core::backup::RestoreProgress| {
                                progress_ctl.progress(p.entries_done, None, Some("entries"), None);
                            }),
                            max_entries: 0,
                            max_total_bytes: 0,
                        },
                    )
                })
                .await
                .map_err(|e| {
                    JobFailure::Error(ProtocolError::new(
                        ErrorCode::InternalError,
                        format!("restore task failed: {e}"),
                    ))
                })?;
                match restore {
                    Ok(outcome) => {
                        ctl.progress(
                            outcome.restored_files,
                            Some(outcome.restored_files.max(1)),
                            Some("entries"),
                            Some(&format!(
                                "restored {} files ({} bytes)",
                                outcome.restored_files, outcome.restored_bytes
                            )),
                        );
                        Ok(())
                    }
                    Err(zamin_core::error::CoreError::Cancelled) => Err(JobFailure::Cancelled),
                    Err(e) => Err(JobFailure::Error(crate::engine::to_protocol(&e))),
                }
            },
        ))
    }

    // --- software catalog & creation (protocol spec §7b) ---------------

    /// Created servers' files live under `<data>/instances/<id>` — never
    /// under `<data>/servers/<id>`, which is the daemon's runtime state
    /// (runtime records, per-server config) and must not be visible as a
    /// server root.
    fn instances_dir(&self) -> PathBuf {
        self.inner.data_dir.join("instances")
    }

    fn fill_client(&self) -> zamin_core::software::FillClient {
        zamin_core::software::FillClient::new(&self.inner.catalog_url)
    }

    pub async fn catalog_list(&self) -> zamin_protocol::software::CatalogListResult {
        use zamin_protocol::software::{CatalogEntry, CatalogListResult};
        CatalogListResult {
            entries: zamin_core::software::CATALOG
                .iter()
                .map(|e| CatalogEntry {
                    id: e.id.to_owned(),
                    name: e.name.to_owned(),
                    description: e.description.to_owned(),
                })
                .collect(),
        }
    }

    pub async fn catalog_versions(
        &self,
        project: &str,
    ) -> Result<zamin_protocol::software::CatalogVersionsResult, EngineError> {
        use zamin_protocol::software::{CatalogVersion, CatalogVersionsResult};
        let entry = zamin_core::software::entry(project).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::CatalogNotFound,
                format!("The software catalog has no entry {project:?}."),
            )
        })?;
        let client = self.fill_client();
        let versions = tokio::task::spawn_blocking(move || client.versions(entry.project))
            .await
            .map_err(|e| internal(&format!("catalog task failed: {e}")))?
            .map_err(|e| EngineError::Protocol(to_protocol(&e)))?;
        Ok(CatalogVersionsResult {
            project: project.to_owned(),
            versions: versions
                .into_iter()
                .map(|v| CatalogVersion {
                    id: v.id,
                    java_major: v.java_major,
                })
                .collect(),
        })
    }

    pub async fn catalog_builds(
        &self,
        project: &str,
        version: &str,
    ) -> Result<zamin_protocol::software::CatalogBuildsResult, EngineError> {
        use zamin_protocol::software::{CatalogBuild, CatalogBuildsResult};
        let entry = zamin_core::software::entry(project).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::CatalogNotFound,
                format!("The software catalog has no entry {project:?}."),
            )
        })?;
        let client = self.fill_client();
        let project_static = entry.project;
        let version_owned = version.to_owned();
        let (builds, java_major) = tokio::task::spawn_blocking(move || {
            let builds = client.builds(project_static, &version_owned)?;
            // The version's Java requirement rides along (one extra
            // metadata call); if it cannot be fetched, the local table
            // decides and the daemon still derives it at creation.
            let java_major = client
                .version_java_major(project_static, &version_owned)
                .ok()
                .flatten()
                .or_else(|| zamin_core::java::required_major(&version_owned));
            Ok::<_, zamin_core::error::CoreError>((builds, java_major))
        })
        .await
        .map_err(|e| internal(&format!("catalog task failed: {e}")))?
        .map_err(|e| EngineError::Protocol(to_protocol(&e)))?;
        Ok(CatalogBuildsResult {
            project: project.to_owned(),
            version: version.to_owned(),
            java_major,
            builds: builds
                .into_iter()
                .map(|b| CatalogBuild {
                    id: b.id,
                    channel: b.channel,
                    time: b.time,
                    download: zamin_protocol::software::CatalogDownload {
                        name: b.download.name,
                        sha256: b.download.sha256,
                        size: b.download.size,
                        url: b.download.url,
                    },
                })
                .collect(),
        })
    }

    /// `server.create`: resolve the requested build, then run the creation
    /// as a job — stamp the template, download and verify the jar, write
    /// the per-server configuration, register. Every failure before
    /// registration cleans the instance directory away: a failed creation
    /// leaves nothing behind.
    pub async fn create_server(
        &self,
        params: zamin_protocol::software::ServerCreateParams,
    ) -> Result<zamin_protocol::jobs::Job, EngineError> {
        use zamin_core::software::{template, DEFAULT_TEMPLATE_ID};

        let server_id = ServerId::parse(&params.server_id)
            .map_err(|e| EngineError::Protocol(to_protocol(&e)))?;
        let display_name = params
            .display_name
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| server_id.to_string());
        let template_id = params
            .template_id
            .clone()
            .unwrap_or_else(|| DEFAULT_TEMPLATE_ID.to_owned());

        // Fast, synchronous rejections before any job exists: an unknown
        // project or template is a bad request; an occupied id is an id
        // conflict — the caller learns in the same request, not from a
        // job event later.
        let entry = zamin_core::software::entry(&params.project).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::CatalogNotFound,
                format!(
                    "The software catalog has no entry {:?}; ask for catalog.list.",
                    params.project
                ),
            )
        })?;
        if template(&template_id).is_none() {
            return Err(ProtocolError::new(
                ErrorCode::ProtocolInvalidRequest,
                format!("Unknown creation template {template_id:?}."),
            )
            .into());
        }
        if self.registry_lock().get(&server_id).is_some() {
            return Err(ProtocolError::new(
                ErrorCode::ServerIdExists,
                format!("Server id {server_id} is already registered."),
            )
            .into());
        }
        let instance_root = self.instances_dir().join(server_id.as_str());
        if instance_root.exists() {
            return Err(ProtocolError::new(
                ErrorCode::ServerIdExists,
                format!(
                    "A directory for server {server_id} already exists at {instance_root:?} but it is not registered; remove it or register it instead."
                ),
            )
            .into());
        }

        // Resolve the build (and the version's Java requirement) up
        // front: an unknown version/build or an unreachable catalog is a
        // typed rejection now, not a job that fails 200 ms in.
        let client = self.fill_client();
        let project = entry.project;
        let version = params.version.clone();
        let wanted_build = params.build;
        let (build, java_major) = tokio::task::spawn_blocking(move || {
            let builds = client.builds(project, &version)?;
            let build =
                match wanted_build {
                    Some(id) => builds.into_iter().find(|b| b.id == id).ok_or_else(|| {
                        zamin_core::error::CoreError::Http {
                            url: format!("catalog: {project} {version} build {id}"),
                            status: 404,
                            reason: "build not found".to_owned(),
                        }
                    })?,
                    None => builds.into_iter().next().ok_or_else(|| {
                        zamin_core::error::CoreError::Http {
                            url: format!("catalog: {project} {version}"),
                            status: 404,
                            reason: "no builds published".to_owned(),
                        }
                    })?,
                };
            let java_major = client
                .version_java_major(project, &version)
                .ok()
                .flatten()
                .or_else(|| zamin_core::java::required_major(&version));
            Ok::<_, zamin_core::error::CoreError>((build, java_major))
        })
        .await
        .map_err(|e| internal(&format!("build resolution task failed: {e}")))?
        .map_err(|e| EngineError::Protocol(to_protocol(&e)))?;

        let engine = self.clone();
        let sid = server_id.clone();
        let root = instance_root.clone();
        let data_dir = self.inner.data_dir.clone();
        Ok(self.inner.jobs.spawn(
            zamin_protocol::jobs::JobKind::ServerCreate,
            Some(server_id.to_string()),
            move |ctl| async move {
                if ctl.cancelled() {
                    return Err(JobFailure::Cancelled);
                }
                ctl.progress(0, None, None, Some("preparing the server directory"));
                std::fs::create_dir_all(&root).map_err(|source| {
                    JobFailure::Error(to_protocol(&zamin_core::error::CoreError::Io {
                        path: root.clone(),
                        source,
                    }))
                })?;

                // Stamp the template (never overwrites; the directory is
                // fresh by the pre-checks above).
                if ctl.cancelled() {
                    let _ = std::fs::remove_dir_all(&root);
                    return Err(JobFailure::Cancelled);
                }
                ctl.progress(0, None, None, Some("writing server files"));
                if let Err(e) = zamin_core::software::stamp_template(&root, &template_id) {
                    let _ = std::fs::remove_dir_all(&root);
                    return Err(JobFailure::Error(to_protocol(&e)));
                }
                if let Some(port) = params.port {
                    patch_stamped_port(&root, port);
                }

                // Download + verify. Cancelled downloads and checksum
                // mismatches leave nothing behind (the downloader owns
                // that guarantee); the instance dir goes too — creation
                // is all-or-nothing.
                let progress_ctl = ctl.clone();
                let cancel_flag = ctl.cancel_flag();
                let name = build.download.name.clone();
                let sha = build.download.sha256.clone();
                let url = build.download.url.clone();
                let size = build.download.size;
                let download_root = root.clone();
                ctl.progress(0, size, Some("bytes"), Some(&format!("downloading {name}")));
                let options = zamin_core::software::DownloadOptions {
                    cancel: cancel_flag,
                    progress: Some(Arc::new(
                        move |p: zamin_core::software::DownloadProgress| {
                            progress_ctl.progress(p.bytes_done, p.total, Some("bytes"), None);
                        },
                    )),
                };
                let outcome = tokio::task::spawn_blocking(move || {
                    zamin_core::software::download_to_dir(
                        &url,
                        &download_root,
                        "server.jar",
                        Some(&sha),
                        &options,
                    )
                })
                .await
                .map_err(|e| {
                    let _ = std::fs::remove_dir_all(&root);
                    JobFailure::Error(ProtocolError::new(
                        ErrorCode::InternalError,
                        format!("download task failed: {e}"),
                    ))
                })?;
                match outcome {
                    Ok(outcome) => ctl.progress(
                        outcome.size,
                        Some(outcome.size.max(1)),
                        Some("bytes"),
                        Some(&format!(
                            "verified {name} (sha256 {}…)",
                            &outcome.sha256[..8]
                        )),
                    ),
                    Err(zamin_core::error::CoreError::Cancelled) => {
                        let _ = std::fs::remove_dir_all(&root);
                        return Err(JobFailure::Cancelled);
                    }
                    Err(e) => {
                        let _ = std::fs::remove_dir_all(&root);
                        return Err(JobFailure::Error(to_protocol(&e)));
                    }
                }

                // Per-server configuration: jar path, the version this
                // server runs, the Java requirement derived from the
                // catalog, and the desired port.
                if ctl.cancelled() {
                    let _ = std::fs::remove_dir_all(&root);
                    return Err(JobFailure::Cancelled);
                }
                ctl.progress(0, None, None, Some("writing the server configuration"));
                let config_path = data_dir
                    .join("servers")
                    .join(sid.as_str())
                    .join("config.toml");
                let config = zamin_core::config::ServerConfigFile {
                    schema_version: zamin_core::config::CONFIG_SCHEMA_VERSION,
                    display_name: Some(display_name.clone()),
                    jar: Some("server.jar".to_owned()),
                    settings: zamin_core::config::ServerSettingsDefaults {
                        mc_version: Some(params.version.clone()),
                        java_major_required: java_major,
                        port: params.port,
                        ..Default::default()
                    },
                };
                if let Err(e) = zamin_core::config::save_server(&config_path, &config) {
                    let _ = std::fs::remove_dir_all(&root);
                    return Err(JobFailure::Error(to_protocol(&e)));
                }

                // Registration is the last step and the point of no
                // return: from here the server exists (marker, registry,
                // `registered` event), so failures no longer clean up.
                if ctl.cancelled() {
                    let _ = std::fs::remove_dir_all(&root);
                    return Err(JobFailure::Cancelled);
                }
                ctl.progress(0, None, None, Some("registering the server"));
                engine
                    .register_server(sid, display_name, root)
                    .await
                    .map_err(job_failure)?;
                Ok(())
            },
        ))
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

/// Map an engine error into a job failure (typed errors pass through).
fn job_failure(error: EngineError) -> JobFailure {
    match error {
        EngineError::Protocol(pe) => JobFailure::Error(pe),
        other => JobFailure::Error(ProtocolError::new(
            ErrorCode::InternalError,
            other.to_string(),
        )),
    }
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

/// Point the stamped `server.properties` at the requested port. The file
/// was written seconds ago by the template stamp; a missing line means a
/// template drift and is left alone (the layered setting stays the
/// authoritative desired value).
fn patch_stamped_port(root: &Path, port: u16) {
    let path = root.join("server.properties");
    if let Ok(content) = std::fs::read_to_string(&path) {
        let patched = {
            let mut found = false;
            let mut out = String::new();
            for line in content.lines() {
                if line.trim_start().starts_with("server-port=") {
                    out.push_str(&format!("server-port={port}\n"));
                    found = true;
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            if !found {
                out.push_str(&format!("server-port={port}\n"));
            }
            out
        };
        let _ = zamin_core::fsops::atomic_write(&path, patched.as_bytes());
    }
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
