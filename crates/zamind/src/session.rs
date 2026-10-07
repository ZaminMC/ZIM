//! Session handling: one connected client. Handshake first, then request
//! dispatch, plus a forwarder task per subscription.
//!
//! The connection is split: the session loop owns the read half; every
//! writer (replies, stream forwarders) feeds one bounded outbound queue
//! drained by a single writer task. A parked receive can therefore never
//! block notification delivery.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::sync::mpsc;
use zamin_core::server::ServerId;
use zamin_ipc::Connection;
use zamin_protocol::backups::{
    BackupCreateParams, BackupCreateResult, BackupRestoreParams, BackupRestoreResult,
    BackupsListParams, BackupsListResult,
};
use zamin_protocol::envelope::{IncomingMessage, Request, RequestId, Response};
use zamin_protocol::error::{ErrorCode, ProtocolError};
use zamin_protocol::handshake::{capabilities, HelloParams, HelloResult};
use zamin_protocol::jobs::{CancelJobParams, GetJobParams, JobKind, ListJobsResult};
use zamin_protocol::methods;
use zamin_protocol::server::{
    EmptyResult, GetServerParams, ListServersResult, RegisterServerParams, RegisterServerResult,
    RemoveServerParams, ServerIdParams, UpdateServerParams,
};
use zamin_protocol::streams::{SubscribeParams, UnsubscribeParams};

use crate::audit::{Audit, AUDITED_METHODS};
use crate::engine::{Engine, EngineError, LifecycleKind};

/// Request-ID dedupe window (protocol spec §3): bounded, TTL'd, a retry
/// safety net — not a transaction log.
const REQUEST_CACHE_TTL: Duration = Duration::from_secs(600);
const REQUEST_CACHE_CAP: usize = 1024;
const OUTBOUND_QUEUE: usize = 256;

/// Shared outbound queue: everything destined for the wire.
#[derive(Clone)]
struct Outbound {
    tx: mpsc::Sender<Bytes>,
}

impl Outbound {
    async fn send(&self, payload: Bytes, what: &str) {
        if self.tx.send(payload).await.is_err() {
            tracing::warn!("outbound queue closed while sending {what}");
        }
    }

    async fn reply(&self, response: Response) {
        if let Ok(payload) = serde_json::to_vec(&response) {
            self.send(payload.into(), "response").await;
        }
    }
}

pub async fn serve(connection: Connection, engine: Engine, audit: Audit) -> Result<(), String> {
    let (mut write_half, mut read_half) = connection.split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel(OUTBOUND_QUEUE);
    let outbound = Outbound { tx: outbound_tx };

    // The writer task is the only sender on the wire.
    let writer = tokio::spawn(async move {
        while let Some(frame) = outbound_rx.recv().await {
            if write_half.send(frame).await.is_err() {
                break;
            }
        }
    });

    let result = session_loop(&mut read_half, &outbound, &engine, &audit).await;

    // Closing the outbound queue ends the writer task.
    drop(outbound);
    let _ = writer.await;
    tracing::info!("session closed");
    result
}

async fn session_loop(
    read_half: &mut zamin_ipc::ConnectionReadHalf,
    outbound: &Outbound,
    engine: &Engine,
    audit: &Audit,
) -> Result<(), String> {
    let mut hello_done = false;
    // The hello's ClientInfo: the audit's honest actor field (ADR-0011 —
    // the agent gates remote identities before the daemon sees them).
    let mut client_info: Option<(String, String)> = None;
    let cache = RequestCache::default();
    let mut live_subscriptions: Vec<String> = Vec::new();

    let result = loop {
        let frame = match read_half.recv().await {
            Ok(Some(frame)) => frame,
            Ok(None) => break Ok(()),
            Err(e) => break Err(format!("receive failed: {e}")),
        };

        let value: serde_json::Value = match serde_json::from_slice(&frame) {
            Ok(value) => value,
            Err(e) => {
                // JSON-RPC 2.0 §4.1: an unreadable id is replied to with a
                // Null id, never a guessed one.
                outbound
                    .reply(Response::err(
                        RequestId::Null,
                        ProtocolError::new(
                            ErrorCode::ProtocolInvalidRequest,
                            format!("The message is not valid JSON: {e}."),
                        ),
                    ))
                    .await;
                continue;
            }
        };

        let Some(IncomingMessage::Request(request)) = IncomingMessage::parse(&value) else {
            let has_method = value.get("method").is_some();
            let has_id_key = value.get("id").is_some();
            if has_method && has_id_key {
                // It *tried* to be a request but the id is unreadable (e.g.
                // an object or a float). JSON-RPC 2.0 §4.1: reply with a
                // typed error carrying a Null id. Genuine notifications
                // (method without id) get no reply, per JSON-RPC §4.2.
                outbound
                    .reply(Response::err(
                        RequestId::Null,
                        ProtocolError::new(
                            ErrorCode::ProtocolInvalidRequest,
                            "The request id must be a number or a string.",
                        ),
                    ))
                    .await;
            }
            // Anything else (notifications, responses, stray shapes) is not
            // ours to judge; v0 has client→daemon requests only.
            continue;
        };

        if !hello_done {
            hello_done = match handle_hello(&request, outbound).await {
                Ok(info) => {
                    client_info = Some(info);
                    audit.record(
                        &request.method,
                        None,
                        "ok",
                        client_info
                            .as_ref()
                            .map(|(name, version)| (name.as_str(), version.as_str())),
                    );
                    true
                }
                Err(response) => {
                    audit.record(&request.method, None, "rejected", None);
                    outbound.reply(response).await;
                    break Err("handshake failed".to_owned());
                }
            };
            continue;
        }

        if request.method == methods::STREAMS_SUBSCRIBE {
            match handle_subscribe(&request, engine, outbound).await {
                Ok(subscription_id) => live_subscriptions.push(subscription_id),
                Err(response) => outbound.reply(response).await,
            }
            continue;
        }

        let request_key = request
            .parse_params::<serde_json::Value>()
            .ok()
            .and_then(|v| {
                v["requestId"]
                    .as_str()
                    .and_then(|s| s.parse::<uuid::Uuid>().ok())
                    .map(|u| u.as_u128())
            });

        if let Some(key) = request_key {
            if let Some(cached) = cache.lookup(key) {
                outbound
                    .reply(Response::ok(request.id.clone(), cached))
                    .await;
                continue;
            }
        }

        let response = dispatch(&request, engine, audit).await;
        if AUDITED_METHODS.contains(&request.method.as_str()) {
            let outcome = match &response.error {
                Some(error) => error.code.as_str(),
                None => "ok",
            };
            let params = request.parse_params::<serde_json::Value>().ok();
            let server_id = params
                .as_ref()
                .and_then(|v| v["serverId"].as_str())
                .map(str::to_owned);
            let detail = params
                .as_ref()
                .and_then(|v| v["jobId"].as_str())
                .map(str::to_owned);
            audit.record(
                &request.method,
                server_id.as_deref().or(detail.as_deref()),
                outcome,
                client_info
                    .as_ref()
                    .map(|(name, version)| (name.as_str(), version.as_str())),
            );
        }
        if let (Some(key), Some(result)) = (request_key, response.result.clone()) {
            cache.remember(key, result);
        }
        outbound.reply(response).await;
    };

    for id in live_subscriptions {
        engine.hub().unsubscribe(&id);
    }
    result
}

/// Mandatory first exchange (protocol spec §2). Anything else before it is
/// a hard, actionable error. Returns the client identity for the audit log.
#[allow(clippy::result_large_err)] // response envelopes are written once, to the wire
async fn handle_hello(
    request: &Request,
    outbound: &Outbound,
) -> Result<(String, String), Response> {
    if request.method != methods::DAEMON_HELLO {
        return Err(Response::err(
            request.id.clone(),
            ProtocolError::new(
                ErrorCode::ProtocolVersionUnsupported,
                "The first exchange must be daemon.hello.",
            ),
        ));
    }
    let params: HelloParams = match request.parse_params() {
        Ok(params) => params,
        Err(e) => {
            return Err(Response::err(
                request.id.clone(),
                ProtocolError::new(
                    ErrorCode::InternalError,
                    format!("Unreadable hello params: {e}."),
                ),
            ))
        }
    };
    if params.protocol != zamin_protocol::PROTOCOL_VERSION {
        return Err(Response::err(
            request.id.clone(),
            ProtocolError::new(
                ErrorCode::ProtocolVersionUnsupported,
                format!(
                    "Daemon speaks protocol {}; client requested protocol {}.",
                    zamin_protocol::PROTOCOL_VERSION,
                    params.protocol
                ),
            )
            .with_context("daemonProtocol", zamin_protocol::PROTOCOL_VERSION)
            .with_context("clientProtocol", params.protocol),
        ));
    }
    let result = HelloResult {
        protocol: zamin_protocol::PROTOCOL_VERSION,
        protocol_min: zamin_protocol::PROTOCOL_VERSION,
        protocol_max: zamin_protocol::PROTOCOL_VERSION,
        daemon: zamin_protocol::handshake::DaemonInfo {
            name: crate::DAEMON_NAME.to_owned(),
            version: crate::DAEMON_VERSION.to_owned(),
        },
        capabilities: vec![
            capabilities::SERVER_LIFECYCLE.to_owned(),
            capabilities::STREAMS.to_owned(),
            capabilities::JOBS.to_owned(),
        ],
    };
    tracing::info!(
        client = %params.client.name,
        version = %params.client.version,
        "client connected"
    );
    let identity = (params.client.name.clone(), params.client.version.clone());
    let payload = match serde_json::to_value(result) {
        Ok(payload) => payload,
        Err(e) => {
            return Err(Response::err(
                request.id.clone(),
                ProtocolError::new(
                    ErrorCode::InternalError,
                    format!("Handshake result serialization failed: {e}."),
                ),
            ))
        }
    };
    outbound
        .reply(Response::ok(request.id.clone(), payload))
        .await;
    Ok(identity)
}

#[allow(clippy::result_large_err)] // response envelopes are written once, to the wire
async fn handle_subscribe(
    request: &Request,
    engine: &Engine,
    outbound: &Outbound,
) -> Result<String, Response> {
    let params: SubscribeParams = request.parse_params().map_err(|e| {
        Response::err(
            request.id.clone(),
            ProtocolError::new(
                ErrorCode::InternalError,
                format!("Unreadable subscribe params: {e}."),
            ),
        )
    })?;

    let (result, subscription) = engine
        .subscribe(params.stream, params.server_id, params.cursor)
        .await
        .map_err(|e| dispatch_error(request.id.clone(), e))?;

    // Forwarder: hub queue → outbound queue. Both are bounded; the hub
    // degrades slow consumers, so this task can never stall the daemon
    // (ADR-0006).
    let forward_outbound = outbound.clone();
    tokio::spawn(async move {
        let mut receiver = subscription.receiver;
        while let Some(notification) = receiver.recv().await {
            let note = zamin_protocol::envelope::Notification::new(
                methods::STREAMS_NOTIFICATION,
                serde_json::to_value(notification).ok(),
            );
            if let Ok(payload) = serde_json::to_vec(&note) {
                forward_outbound.send(payload.into(), "notification").await;
            }
        }
    });

    let subscription_id = result.subscription_id.clone();
    let payload = match serde_json::to_value(result) {
        Ok(payload) => payload,
        Err(e) => {
            return Err(Response::err(
                request.id.clone(),
                ProtocolError::new(
                    ErrorCode::InternalError,
                    format!("Subscribe result serialization failed: {e}."),
                ),
            ))
        }
    };
    outbound
        .reply(Response::ok(request.id.clone(), payload))
        .await;
    Ok(subscription_id)
}

#[allow(clippy::result_large_err)] // response envelopes are written once, to the wire
async fn dispatch(request: &Request, engine: &Engine, audit: &Audit) -> Response {
    let id = request.id.clone();
    match request.method.as_str() {
        methods::DAEMON_STATUS => Response::ok(id, engine.daemon_status().await),
        methods::SERVER_LIST => {
            let servers = engine.list_servers().await;
            json_ok(id, ListServersResult { servers })
        }
        methods::SERVER_GET => match parse_server_id::<GetServerParams>(request) {
            Ok((server_id, _)) => match engine.get_server(&server_id).await {
                Ok(details) => json_ok(id, details),
                Err(e) => dispatch_error(id, e),
            },
            Err(response) => response,
        },
        methods::SERVER_REGISTER => {
            let params: RegisterServerParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    match engine
                        .register_server(server_id, params.display_name, params.root_path.into())
                        .await
                    {
                        Ok(server) => json_ok(id, RegisterServerResult { server }),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::SERVER_UPDATE => {
            let params: UpdateServerParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    match engine
                        .rename_server(&server_id, params.display_name.unwrap_or_default())
                        .await
                    {
                        Ok(details) => json_ok(id, details),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::SERVER_REMOVE => match parse_server_id::<RemoveServerParams>(request) {
            Ok((server_id, _)) => match engine.remove_server(&server_id).await {
                Ok(()) => json_ok(id, EmptyResult {}),
                Err(e) => dispatch_error(id, e),
            },
            Err(response) => response,
        },
        methods::SERVER_START => lifecycle(request, engine, LifecycleKind::Start).await,
        methods::SERVER_STOP => lifecycle(request, engine, LifecycleKind::Stop).await,
        methods::SERVER_RESTART => lifecycle(request, engine, LifecycleKind::Restart).await,
        methods::SERVER_KILL => lifecycle(request, engine, LifecycleKind::Kill).await,
        methods::SERVER_STDIN => {
            let params: zamin_protocol::server::StdinParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.write_stdin(&server_id, params.line).await {
                    Ok(()) => json_ok(id, EmptyResult {}),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::JOBS_LIST => json_ok(
            id,
            ListJobsResult {
                jobs: engine.jobs().list(),
            },
        ),
        methods::JOBS_GET => {
            let params: GetJobParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match engine.jobs().get(params.job_id) {
                Ok(job) => json_ok(id, job),
                Err(e) => Response::err(id, e),
            }
        }
        methods::JOBS_CANCEL => {
            let params: CancelJobParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match engine.jobs().cancel(params.job_id) {
                Ok(job) => json_ok(id, job),
                Err(e) => Response::err(id, e),
            }
        }
        methods::BACKUP_CREATE => {
            let params: BackupCreateParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.backup_create(&server_id, params.label).await {
                    Ok(job) => json_ok(
                        id,
                        BackupCreateResult {
                            kind: JobKind::BackupCreate,
                            job,
                        },
                    ),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::BACKUP_RESTORE => {
            let params: BackupRestoreParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.backup_restore(&server_id, params.backup_id).await {
                    Ok(job) => json_ok(
                        id,
                        BackupRestoreResult {
                            kind: JobKind::BackupRestore,
                            job,
                        },
                    ),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::BACKUPS_LIST => {
            let params: BackupsListParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.backups_list(&server_id).await {
                    Ok(result) => json_ok(
                        id,
                        BackupsListResult {
                            backups: result.backups,
                        },
                    ),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::SCHEDULES_LIST => {
            let params: zamin_protocol::schedules::SchedulesListParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.schedules_list(&server_id).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::SCHEDULES_CREATE => {
            let params: zamin_protocol::schedules::SchedulesCreateParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.schedules_create(&server_id, &params).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::SCHEDULES_UPDATE => {
            let params: zamin_protocol::schedules::SchedulesUpdateParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.schedules_update(&server_id, &params).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::SCHEDULES_DELETE => {
            let params: zamin_protocol::schedules::SchedulesDeleteParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine
                    .schedules_delete(&server_id, &params.schedule_id)
                    .await
                {
                    Ok(()) => json_ok(id, EmptyResult {}),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::AUDIT_LIST => {
            // The audit's read side (ADR-0011): newest-first, paged, and
            // like every read it is not itself audited — a listing floods
            // the file without making the system safer.
            let params: zamin_protocol::audit::AuditListParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            let read = audit.read(params.limit, params.offset);
            json_ok(id, read)
        }
        // Server configuration surfaces (founder §37–39, ADR-0019). The
        // write (config.set) is audited like every mutation; the reads
        // (config.get, network.status) are observation.
        methods::CONFIG_GET => {
            let params: zamin_protocol::config::ConfigGetParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.config_get(&server_id).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::CONFIG_SET => {
            let params: zamin_protocol::config::ConfigSetParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine
                    .config_set(&server_id, params.display_name, params.jar, params.settings)
                    .await
                {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::NETWORK_STATUS => {
            let params: zamin_protocol::config::NetworkStatusParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.network_status(&server_id).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        // Publish (founder §40–47, §74, ADR-0017). Mutations ride the
        // AUDITED_METHODS list; the reads (config.get, preview, state,
        // providers.list) are observation like every other read.
        methods::PUBLISH_CONFIG_GET => {
            let params: zamin_protocol::publish::PublishConfigGetParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.publish_config_get(&server_id).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PUBLISH_CONFIG_SET => {
            let params: zamin_protocol::publish::PublishConfigSetParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    match engine.publish_config_set(&server_id, &params.config).await {
                        Ok(result) => json_ok(id, result),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PUBLISH_PROVIDERS_LIST => json_ok(id, engine.publish_providers_list()),
        methods::PUBLISH_PREVIEW => {
            let params: zamin_protocol::publish::PublishPreviewParams = match request.parse_params()
            {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.publish_preview(&server_id).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PUBLISH_EXECUTE => {
            let params: zamin_protocol::publish::PublishExecuteParams = match request.parse_params()
            {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine
                    .publish_execute(&server_id, params.confirm_unsafe.unwrap_or(false))
                    .await
                {
                    Ok(job) => json_ok(id, zamin_protocol::publish::PublishExecuteResult { job }),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PUBLISH_STATE => {
            let params: zamin_protocol::publish::PublishStateParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.publish_state(&server_id).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PUBLISH_REVIEW_SET => {
            let params: zamin_protocol::publish::PublishReviewSetParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine
                    .publish_review_set(&server_id, &params.file, &params.kind, params.reviewed)
                    .await
                {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::CATALOG_LIST => json_ok(id, engine.catalog_list().await),
        methods::CATALOG_VERSIONS => {
            let params: zamin_protocol::software::CatalogVersionsParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match engine.catalog_versions(&params.project).await {
                Ok(result) => json_ok(id, result),
                Err(e) => dispatch_error(id, e),
            }
        }
        methods::CATALOG_BUILDS => {
            let params: zamin_protocol::software::CatalogBuildsParams = match request.parse_params()
            {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match engine
                .catalog_builds(&params.project, &params.version)
                .await
            {
                Ok(result) => json_ok(id, result),
                Err(e) => dispatch_error(id, e),
            }
        }
        methods::SERVER_CREATE => {
            let params: zamin_protocol::software::ServerCreateParams = match request.parse_params()
            {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match engine.create_server(params).await {
                Ok(job) => json_ok(
                    id,
                    zamin_protocol::software::ServerCreateResult {
                        kind: JobKind::ServerCreate,
                        job,
                    },
                ),
                Err(e) => dispatch_error(id, e),
            }
        }
        methods::JAVA_LIST => json_ok(id, engine.java_list().await),
        methods::JAVA_INSTALL => {
            let params: zamin_protocol::java::JavaInstallParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match engine.java_install(params.major_version).await {
                Ok(job) => json_ok(
                    id,
                    zamin_protocol::java::JavaInstallResult {
                        kind: JobKind::JavaInstall,
                        job,
                    },
                ),
                Err(e) => dispatch_error(id, e),
            }
        }
        methods::PLUGINS_SEARCH => {
            let params: zamin_protocol::plugins::PluginsSearchParams = match request.parse_params()
            {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    let limit = params.limit.unwrap_or(20);
                    match engine
                        .plugins_search(&server_id, &params.query, limit)
                        .await
                    {
                        Ok(result) => json_ok(id, result),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PLUGINS_VERSIONS => {
            let params: zamin_protocol::plugins::PluginsVersionsParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    match engine
                        .plugins_versions(&server_id, &params.project_id)
                        .await
                    {
                        Ok(result) => json_ok(id, result),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PLUGINS_INSTALLED => {
            let params: zamin_protocol::plugins::PluginsInstalledParams =
                match request.parse_params() {
                    Ok(params) => params,
                    Err(e) => return unreadable(id, e),
                };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.plugins_installed(&server_id).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PLUGINS_INSTALL => {
            let params: zamin_protocol::plugins::PluginsInstallParams = match request.parse_params()
            {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine
                    .plugins_install(
                        &server_id,
                        &params.project_id,
                        params.version_id,
                        params.replace,
                        params.retire_file,
                    )
                    .await
                {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PLUGINS_DELETE => {
            let params: zamin_protocol::plugins::PluginsDeleteParams = match request.parse_params()
            {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.plugins_delete(&server_id, &params.file_name).await {
                    Ok(()) => json_ok(id, zamin_protocol::server::EmptyResult {}),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PLUGINS_UPDATES => {
            let params: zamin_protocol::plugins::PluginsUpdatesParams = match request.parse_params()
            {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.plugins_updates(&server_id).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::LOGS_RANGE => {
            let params: zamin_protocol::logs::LogRangeParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    let max_lines = params.max_lines.unwrap_or(200);
                    match engine
                        .log_range(&server_id, max_lines, params.before_offset)
                        .await
                    {
                        Ok(result) => json_ok(id, result),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::METRICS_RANGE => {
            let params: zamin_protocol::metrics::MetricsRangeParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    let max = params
                        .max_samples
                        .unwrap_or(120)
                        .clamp(1, zamin_protocol::metrics::METRICS_RANGE_MAX_SAMPLES);
                    match engine.metrics_range(&server_id, max) {
                        Ok(result) => json_ok(id, result),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::FILES_LIST => {
            let params: zamin_protocol::files::FilesListParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    let offset = params.offset.unwrap_or(0);
                    let limit = params
                        .limit
                        .unwrap_or(zamin_protocol::files::FILES_LIST_DEFAULT_LIMIT)
                        .clamp(1, zamin_protocol::files::FILES_LIST_MAX_LIMIT);
                    match engine
                        .files_list(&server_id, &params.path, offset, limit)
                        .await
                    {
                        Ok(result) => json_ok(id, result),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::FILES_READ => {
            let params: zamin_protocol::files::FilesReadParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    match engine
                        .files_read(&server_id, &params.path, params.offset, params.max_bytes)
                        .await
                    {
                        Ok(result) => json_ok(id, result),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::FILES_WRITE => {
            let params: zamin_protocol::files::FilesWriteParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    match engine
                        .files_write(&server_id, params.staging_id, &params.content)
                        .await
                    {
                        Ok(result) => json_ok(id, result),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::FILES_COMMIT => {
            let params: zamin_protocol::files::FilesCommitParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    match engine
                        .files_commit(&server_id, &params.staging_id, &params.target)
                        .await
                    {
                        Ok(result) => json_ok(id, result),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::FILES_MKDIR => {
            let params: zamin_protocol::files::FilesMkdirParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.files_mkdir(&server_id, &params.path).await {
                    Ok(()) => json_ok(id, EmptyResult {}),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::FILES_RENAME => {
            let params: zamin_protocol::files::FilesRenameParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => {
                    match engine
                        .files_rename(&server_id, &params.from, &params.to)
                        .await
                    {
                        Ok(()) => json_ok(id, EmptyResult {}),
                        Err(e) => dispatch_error(id, e),
                    }
                }
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::FILES_DELETE => {
            let params: zamin_protocol::files::FilesDeleteParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.files_delete(&server_id, &params.path).await {
                    Ok(()) => json_ok(id, EmptyResult {}),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::PLAYERS_LIST => {
            let params: zamin_protocol::players::PlayersListParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            match ServerId::parse(&params.server_id) {
                Ok(server_id) => match engine.players_list(&server_id).await {
                    Ok(result) => json_ok(id, result),
                    Err(e) => dispatch_error(id, e),
                },
                Err(e) => Response::err(id, crate::engine::to_protocol(&e)),
            }
        }
        methods::STREAMS_UNSUBSCRIBE => {
            let params: UnsubscribeParams = match request.parse_params() {
                Ok(params) => params,
                Err(e) => return unreadable(id, e),
            };
            engine.hub().unsubscribe(&params.subscription_id);
            json_ok(id, EmptyResult {})
        }
        other => Response::err(
            id,
            ProtocolError::new(
                ErrorCode::ProtocolMethodNotFound,
                format!("Method {other:?} does not exist in this daemon's protocol surface."),
            ),
        ),
    }
}

#[allow(clippy::result_large_err)] // response envelopes are written once, to the wire
fn parse_server_id<T: serde::de::DeserializeOwned + ServerIdParamsLike>(
    request: &Request,
) -> Result<(ServerId, T), Response> {
    let params: T = request
        .parse_params()
        .map_err(|e| unreadable(request.id.clone(), e))?;
    let server_id = ServerId::parse(params.server_id_field())
        .map_err(|e| Response::err(request.id.clone(), crate::engine::to_protocol(&e)))?;
    Ok((server_id, params))
}

/// Lets `parse_server_id` read the serverId field without a second parse.
trait ServerIdParamsLike {
    fn server_id_field(&self) -> &str;
}

impl ServerIdParamsLike for GetServerParams {
    fn server_id_field(&self) -> &str {
        &self.server_id
    }
}

impl ServerIdParamsLike for RemoveServerParams {
    fn server_id_field(&self) -> &str {
        &self.server_id
    }
}

async fn lifecycle(request: &Request, engine: &Engine, kind: LifecycleKind) -> Response {
    let id = request.id.clone();
    let params: ServerIdParams = match request.parse_params() {
        Ok(params) => params,
        Err(e) => return unreadable(id, e),
    };
    let server_id = match ServerId::parse(&params.server_id) {
        Ok(server_id) => server_id,
        Err(e) => return Response::err(id, crate::engine::to_protocol(&e)),
    };
    match engine.lifecycle(&server_id, kind).await {
        Ok(result) => json_ok(id, result),
        Err(e) => dispatch_error(id, e),
    }
}

fn json_ok<T: serde::Serialize>(id: RequestId, value: T) -> Response {
    match serde_json::to_value(value) {
        Ok(payload) => Response::ok(id, payload),
        Err(e) => Response::err(
            id,
            ProtocolError::new(
                ErrorCode::InternalError,
                format!("Response serialization failed: {e}."),
            ),
        ),
    }
}

fn dispatch_error(id: RequestId, error: EngineError) -> Response {
    let protocol = match &error {
        EngineError::Protocol(p) => p.clone(),
        EngineError::Hub(_) => ProtocolError::new(
            ErrorCode::InternalError,
            "The requested cursor cannot be served; re-snapshot instead of replaying.",
        ),
        EngineError::Internal(message) => {
            ProtocolError::new(ErrorCode::InternalError, message.clone())
        }
    };
    Response::err(id, protocol)
}

fn unreadable(id: RequestId, error: serde_json::Error) -> Response {
    Response::err(
        id,
        ProtocolError::new(
            ErrorCode::InternalError,
            format!("Unreadable params: {error}."),
        ),
    )
}

/// Bounded, TTL'd dedupe of recently completed mutating requests, keyed by
/// the client's requestId (UUIDv7, compared as its 128-bit value).
struct RequestCache {
    entries: Mutex<HashMap<u128, (Instant, serde_json::Value)>>,
}

impl Default for RequestCache {
    fn default() -> Self {
        RequestCache {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl RequestCache {
    fn lookup(&self, key: u128) -> Option<serde_json::Value> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (stamp, value) = entries.get(&key)?;
        if stamp.elapsed() > REQUEST_CACHE_TTL {
            entries.remove(&key);
            return None;
        }
        Some(value.clone())
    }

    fn remember(&self, key: u128, value: serde_json::Value) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.len() >= REQUEST_CACHE_CAP {
            // Clear on overflow rather than an LRU: the window is a retry
            // safety net, and a 1024-request burst in one session already
            // means something unusual.
            entries.clear();
        }
        entries.insert(key, (Instant::now(), value));
    }
}
