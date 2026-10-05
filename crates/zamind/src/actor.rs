//! The per-server actor (ADR-0005): the sole owner of one server's process,
//! stdin, state machine, and log pipeline. Commands arrive over a bounded
//! channel; everything mutating happens here, serialized — double-start
//! races and interleaved stdin are structurally impossible.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use zamin_core::config::{self, EffectiveSettings, ServerConfigFile};
use zamin_core::error::CoreError;
use zamin_core::logparse;
use zamin_core::platform::{self, ProcessIdentity, Spawned};
use zamin_core::supervisor::state::StateMachine;
use zamin_core::supervisor::LifecycleCommand;
use zamin_protocol::error::{ErrorCode, ProtocolError};
use zamin_protocol::server::{CrashClassification, ServerState};
use zamin_protocol::streams::{LogLevel, LogLine};

use crate::hub::HubHandle;

const TICK: Duration = Duration::from_millis(250);
const GRACEFUL_GRACE: Duration = Duration::from_secs(10);

pub enum ActorCommand {
    Start {
        reply: oneshot::Sender<Result<ServerState, ProtocolError>>,
    },
    Stop {
        reply: oneshot::Sender<Result<ServerState, ProtocolError>>,
    },
    Kill {
        reply: oneshot::Sender<Result<ServerState, ProtocolError>>,
    },
    Restart {
        reply: oneshot::Sender<Result<ServerState, ProtocolError>>,
    },
    WriteStdin {
        line: String,
        reply: oneshot::Sender<Result<(), ProtocolError>>,
    },
    Describe {
        reply: oneshot::Sender<ActorSnapshot>,
    },
    /// Stop if needed, then terminate the actor (server removal).
    Retire {
        reply: oneshot::Sender<Result<(), ProtocolError>>,
    },
}

#[derive(Debug, Clone)]
pub struct ActorSnapshot {
    pub state: ServerState,
}

pub struct Actor {
    server_id: String,
    root: PathBuf,
    runtime_dir: PathBuf,
    global_config_path: PathBuf,
    hub: HubHandle,

    machine: StateMachine,
    spawned: Option<Spawned>,
    identity: Option<ProcessIdentity>,
    stdin: Option<tokio::process::ChildStdin>,
    started_at_ms: Option<i64>,

    // Shutdown-ladder progress, driven by the tick loop so ownership of the
    // child handle never leaves the actor.
    stop_requested_at: Option<Instant>,
    graceful_signaled: bool,

    pending_restart: bool,
    adopted_identity: Option<ProcessIdentity>,
}

impl Actor {
    pub fn new(
        server_id: String,
        root: PathBuf,
        runtime_dir: PathBuf,
        global_config_path: PathBuf,
        hub: HubHandle,
        initial_state: zamin_core::supervisor::state::ServerState,
    ) -> Actor {
        Actor {
            server_id,
            root,
            runtime_dir,
            global_config_path,
            hub,
            machine: StateMachine::new(initial_state),
            spawned: None,
            identity: None,
            stdin: None,
            started_at_ms: None,
            stop_requested_at: None,
            graceful_signaled: false,
            pending_restart: false,
            adopted_identity: None,
        }
    }

    pub fn spawn_task(
        server_id: String,
        root: PathBuf,
        runtime_dir: PathBuf,
        global_config_path: PathBuf,
        hub: HubHandle,
        initial_state: zamin_core::supervisor::state::ServerState,
    ) -> mpsc::Sender<ActorCommand> {
        let (tx, rx) = mpsc::channel(64);
        let actor = Actor::new(
            server_id,
            root,
            runtime_dir,
            global_config_path,
            hub,
            initial_state,
        );
        tokio::spawn(actor.run(rx));
        tx
    }

    async fn run(mut self, mut rx: mpsc::Receiver<ActorCommand>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                command = rx.recv() => {
                    let Some(command) = command else { break };
                    if self.handle(command).await {
                        break; // retired
                    }
                }
                _ = tick.tick() => {
                    self.on_tick().await;
                }
            }
        }
    }

    /// Returns `true` when the actor should terminate (retired).
    async fn handle(&mut self, command: ActorCommand) -> bool {
        match command {
            ActorCommand::Start { reply } => {
                let result = self.start().await;
                let _ = reply.send(result.map(|_| self.wire_state()));
            }
            ActorCommand::Stop { reply } => {
                let result = self.stop(false).await;
                let _ = reply.send(result);
            }
            ActorCommand::Kill { reply } => {
                let result = self.stop(true).await;
                let _ = reply.send(result);
            }
            ActorCommand::Restart { reply } => {
                let result = self.restart().await;
                let _ = reply.send(result);
            }
            ActorCommand::WriteStdin { line, reply } => {
                let result = self.write_stdin(&line).await;
                let _ = reply.send(result);
            }
            ActorCommand::Describe { reply } => {
                let _ = reply.send(ActorSnapshot {
                    state: self.wire_state(),
                });
            }
            ActorCommand::Retire { reply } => {
                if self.spawned.is_some() {
                    let _ = self.stop(true).await;
                    // The child takes up to the ladder to die; the actor
                    // exits after the stop flow reports — the retire reply
                    // acknowledges the request, not the exit.
                }
                self.clear_runtime_record();
                let _ = reply.send(Ok(()));
                return true;
            }
        }
        false
    }

    fn wire_state(&self) -> ServerState {
        self.machine.state().into()
    }

    fn publish_state(&mut self, reason: Option<&str>, error: Option<ProtocolError>) {
        let to = self.wire_state();
        self.hub.publish_event(
            Some(self.server_id.clone()),
            zamin_protocol::streams::CoreEvent::ServerStateChanged {
                server_id: self.server_id.clone(),
                from: to,
                to,
                reason: reason.map(str::to_owned),
                exit_code: None,
                error,
                crash: None,
            },
        );
    }

    fn publish_crash(&self, report: CrashClassification) {
        self.hub.publish_event(
            Some(self.server_id.clone()),
            zamin_protocol::streams::CoreEvent::ServerStateChanged {
                server_id: self.server_id.clone(),
                from: ServerState::Crashed,
                to: ServerState::Crashed,
                reason: Some("unexpected-exit".to_owned()),
                exit_code: report.exit_code,
                error: None,
                crash: Some(report),
            },
        );
    }

    fn publish_transition(&mut self, from: ServerState, reason: Option<String>) {
        let to = self.wire_state();
        self.hub.publish_event(
            Some(self.server_id.clone()),
            zamin_protocol::streams::CoreEvent::ServerStateChanged {
                server_id: self.server_id.clone(),
                from,
                to,
                reason,
                exit_code: None,
                error: None,
                crash: None,
            },
        );
    }

    // --- lifecycle flows ---

    async fn start(&mut self) -> Result<(), ProtocolError> {
        if self.spawned.is_some() {
            return Ok(()); // idempotent: starting/running stays as it is
        }
        let from = self.wire_state();
        self.machine
            .apply(LifecycleCommand::Start)
            .map_err(typed_rejection)?;
        self.publish_transition(from, Some("start-requested".to_owned()));

        let settings = match self.load_effective_settings().await {
            Ok(settings) => settings,
            Err(error) => return Err(self.preflight_failure(error)),
        };
        if let Err(error) = self.preflight(&settings).await {
            return Err(self.preflight_failure(error));
        }

        let java = settings
            .java_path
            .clone()
            .map(Ok)
            .unwrap_or_else(select_java)
            .map_err(|e| {
                self.preflight_failure(CoreError::JavaNotFound {
                    requirement: e.to_string(),
                })
            })?;
        let java_info = tokio::task::spawn_blocking({
            let java = java.clone();
            move || zamin_core::java::inspect(&java)
        })
        .await
        .map_err(|e| as_internal(e.to_string()))?
        .map_err(|e| self.preflight_failure(e))?;

        let jar = {
            let cfg = self.load_server_config().await;
            cfg.jar.unwrap_or_else(|| "server.jar".to_owned())
        };
        let jar_path = self.root.join(&jar);
        if !jar_path.is_file() {
            return Err(self.preflight_failure(CoreError::NotFound { path: jar_path }));
        }

        let mut args: Vec<String> = Vec::new();
        if let Some(min) = settings.min_memory_mb {
            args.push("-Xms".to_owned());
            args.push(format!("{min}M"));
        }
        if let Some(max) = settings.max_memory_mb {
            args.push("-Xmx".to_owned());
            args.push(format!("{max}M"));
        }
        args.extend(settings.extra_jvm_args.clone());
        args.push("-jar".to_owned());
        args.push(jar);
        args.push("nogui".to_owned());

        let spec = zamin_core::platform::SpawnSpec {
            program: java,
            args,
            working_dir: self.root.clone(),
        };
        let mut spawned = tokio::task::spawn_blocking({
            let spec = spec.clone();
            move || platform::process().spawn(&spec)
        })
        .await
        .map_err(|e| as_internal(e.to_string()))?
        .map_err(|e| self.preflight_failure(CoreError::Platform(e)))?;

        let pid = spawned.pid();
        self.identity = platform::process().identity(pid);
        self.stdin = spawned.handle().child().stdin.take();
        self.spawned = Some(spawned);
        self.started_at_ms = Some(now_ms());
        self.machine
            .apply(LifecycleCommand::Spawned)
            .map_err(typed_rejection)?;

        self.persist_runtime_record();
        self.spawn_log_pumps();
        tracing::info!(
            server = %self.server_id,
            pid,
            java = %java_info.version_string,
            "server process spawned"
        );
        Ok(())
    }

    fn preflight_failure(&mut self, error: CoreError) -> ProtocolError {
        let protocol = to_protocol_error(&error);
        self.machine
            .apply(LifecycleCommand::PreflightFailed {
                error: error.to_string(),
            })
            .ok();
        self.publish_state(Some("preflight-failed"), Some(protocol.clone()));
        protocol
    }

    async fn load_effective_settings(&self) -> Result<EffectiveSettings, CoreError> {
        let global = config::load_global(&self.global_config_path)?;
        let per = self.load_server_config().await;
        Ok(config::layer(&global.defaults, &per.settings))
    }

    async fn load_server_config(&self) -> ServerConfigFile {
        let path = self.server_config_path();
        match tokio::task::spawn_blocking(move || config::load_server(&path)).await {
            Ok(Ok(file)) => file,
            Ok(Err(_)) | Err(_) => ServerConfigFile {
                schema_version: config::CONFIG_SCHEMA_VERSION,
                ..ServerConfigFile::default()
            },
        }
    }

    fn server_config_path(&self) -> PathBuf {
        self.runtime_dir.join("config.toml")
    }

    /// Typed pre-spawn checks (ADR-0005). Each failure is independently
    /// reported; no spawn happens on partial failure.
    async fn preflight(&self, settings: &EffectiveSettings) -> Result<(), CoreError> {
        // EULA: a missing or unaccepted eula.txt is the classic first-run
        // trap; catch it before wasting a boot.
        let eula = self.root.join("eula.txt");
        let accepted = tokio::fs::read_to_string(&eula)
            .await
            .map(|c| c.to_ascii_lowercase().contains("eula=true"))
            .unwrap_or(false);
        if !accepted {
            return Err(CoreError::NeedsEula { path: eula });
        }

        if let Some(port) = settings.port {
            zamin_core::net::check_available(port)?;
        }

        // Directory writability: create and remove a probe file.
        let probe = self.root.join(".zamin-preflight-probe");
        tokio::fs::write(&probe, b"ok")
            .await
            .map_err(|source| CoreError::Io {
                path: probe.clone(),
                source,
            })?;
        tokio::fs::remove_file(&probe)
            .await
            .map_err(|source| CoreError::Io {
                path: probe,
                source,
            })?;
        Ok(())
    }

    async fn stop(&mut self, force: bool) -> Result<ServerState, ProtocolError> {
        let has_child = self.spawned.is_some();
        let state = self.machine.state();
        if !has_child {
            return match state {
                zamin_core::supervisor::state::ServerState::Stopping => Ok(self.wire_state()),
                _ => Err(ProtocolError::new(
                    ErrorCode::ServerNotRunning,
                    format!("Server '{}' is not running.", self.server_id),
                )),
            };
        }
        if force {
            // Kill = stop without grace: same machine path, no waiting.
            if let Some(spawned) = self.spawned.as_mut() {
                let _ = spawned.handle().force_kill_tree();
            }
            return Ok(self.wire_state());
        }
        match state {
            zamin_core::supervisor::state::ServerState::Running => {
                let from = self.wire_state();
                self.machine
                    .apply(LifecycleCommand::Stop)
                    .map_err(typed_rejection)?;
                self.publish_transition(from, Some("stop-requested".to_owned()));
                self.stop_requested_at = Some(Instant::now());
                self.graceful_signaled = false;
                if let Some(stdin) = self.stdin.as_mut() {
                    let _ = stdin.write_all(b"stop\n").await;
                    let _ = stdin.flush().await;
                }
                Ok(self.wire_state())
            }
            zamin_core::supervisor::state::ServerState::Stopping => Ok(self.wire_state()),
            _ => Err(ProtocolError::new(
                ErrorCode::ServerNotRunning,
                format!(
                    "Server '{}' cannot stop gracefully from state {:?}; use kill.",
                    self.server_id, state
                ),
            )),
        }
    }

    async fn restart(&mut self) -> Result<ServerState, ProtocolError> {
        match self.machine.state() {
            zamin_core::supervisor::state::ServerState::NotRunning
            | zamin_core::supervisor::state::ServerState::Stopped
            | zamin_core::supervisor::state::ServerState::Crashed
            | zamin_core::supervisor::state::ServerState::FailedPreflight => {
                self.start().await?;
                Ok(self.wire_state())
            }
            zamin_core::supervisor::state::ServerState::Running => {
                self.pending_restart = true;
                self.stop(false).await
            }
            zamin_core::supervisor::state::ServerState::Stopping => {
                self.pending_restart = true;
                Ok(self.wire_state())
            }
            _ => Err(ProtocolError::new(
                ErrorCode::ServerNotRunning,
                format!(
                    "Server '{}' cannot restart from state {:?}.",
                    self.server_id,
                    self.machine.state()
                ),
            )),
        }
    }

    async fn write_stdin(&mut self, line: &str) -> Result<(), ProtocolError> {
        match self.stdin.as_mut() {
            Some(stdin) => {
                stdin
                    .write_all(format!("{line}\n").as_bytes())
                    .await
                    .map_err(|e| as_internal(e.to_string()))?;
                stdin
                    .flush()
                    .await
                    .map_err(|e| as_internal(e.to_string()))?;
                Ok(())
            }
            None => Err(ProtocolError::new(
                ErrorCode::ServerNotRunning,
                format!(
                    "Server '{}' has no console; it was likely adopted after a daemon restart.",
                    self.server_id
                ),
            )),
        }
    }

    /// The tick drives everything that is not a command: child exit
    /// polling, startup validation, the shutdown ladder, and adoption
    /// monitoring. Polling `try_wait` every 250 ms keeps child ownership
    /// inside the actor with no select! borrow gymnastics; exit detection
    /// at that latency is invisible next to a JVM shutdown.
    async fn on_tick(&mut self) {
        // Child exit check first: an exited process is not "validating" or
        // "stopping" anymore.
        if let Some(spawned) = self.spawned.as_mut() {
            if let Ok(Some(status)) = spawned.handle().child().try_wait() {
                self.on_exit(status).await;
                return;
            }
        }
        match self.machine.state() {
            zamin_core::supervisor::state::ServerState::Starting => {
                self.check_startup_validation().await;
            }
            zamin_core::supervisor::state::ServerState::Stopping => {
                self.run_shutdown_ladder().await;
            }
            _ => {}
        }
        // Adopted servers have no child handle; poll identity for death.
        if self.spawned.is_none() {
            let adopted_gone = self
                .adopted_identity
                .as_ref()
                .is_some_and(|identity| !platform::process().is_alive(identity));
            if adopted_gone {
                self.adopted_identity = None;
                tracing::info!(server = %self.server_id, "adopted server exited");
                let from = self.wire_state();
                if self
                    .machine
                    .apply(LifecycleCommand::Crashed { exit_code: 0 })
                    .is_ok()
                {
                    self.publish_transition(from, Some("adopted-process-exited".to_owned()));
                }
            }
        }
    }

    async fn check_startup_validation(&mut self) {
        let validated = self
            .hub
            .log_ring(&self.server_id)
            .iter()
            .rev()
            .take(50)
            .any(|l| logparse::is_startup_complete(&l.line))
            || self.port_is_listening().await;
        if validated {
            let from = self.wire_state();
            if self
                .machine
                .apply(LifecycleCommand::StartupValidated)
                .is_ok()
            {
                self.publish_transition(from, Some("startup-validated".to_owned()));
                tracing::info!(server = %self.server_id, "server is running");
            }
        }
    }

    async fn port_is_listening(&self) -> bool {
        let settings = self.load_effective_settings().await.ok();
        match settings.and_then(|s| s.port) {
            Some(port) => tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok(),
            None => false,
        }
    }

    async fn run_shutdown_ladder(&mut self) {
        let Some(requested_at) = self.stop_requested_at else {
            return;
        };
        let settings = self.load_effective_settings().await.ok();
        let stop_timeout =
            Duration::from_secs(settings.map(|s| s.stop_timeout_secs).unwrap_or(60).max(1) as u64);
        let elapsed = requested_at.elapsed();

        if !self.graceful_signaled && elapsed >= stop_timeout {
            // Ladder step 3: the OS-graceful signal. On a windowless Windows
            // daemon this may be unsupported; the ladder falls through and
            // the log says so (ADR-0005).
            self.graceful_signaled = true;
            if let Some(identity) = &self.identity {
                match platform::process().signal_graceful(identity.pid) {
                    Ok(()) => {
                        tracing::info!(server = %self.server_id, "sent os-level graceful signal")
                    }
                    Err(e) => tracing::warn!(
                        server = %self.server_id,
                        "os-level graceful signal unavailable ({e}); falling through to force"
                    ),
                }
            }
            self.stop_requested_at = Some(Instant::now() - GRACEFUL_GRACE);
            return;
        }

        if self.graceful_signaled && elapsed >= stop_timeout + GRACEFUL_GRACE {
            tracing::warn!(server = %self.server_id, "stop timeout exceeded; forcing tree termination");
            if let Some(spawned) = self.spawned.as_mut() {
                let _ = spawned.handle().force_kill_tree();
            }
        }
    }

    async fn on_exit(&mut self, status: std::process::ExitStatus) {
        let exit_code = status.code().unwrap_or(-1);
        let _ = self.identity.take();
        self.spawned = None;
        self.stdin = None;
        self.stop_requested_at = None;
        self.graceful_signaled = false;
        self.clear_runtime_record();

        let from = self.wire_state();
        let command = match self.machine.state() {
            zamin_core::supervisor::state::ServerState::Starting => {
                LifecycleCommand::StartupFailed { exit_code }
            }
            zamin_core::supervisor::state::ServerState::Stopping => {
                LifecycleCommand::StoppedGracefully
            }
            _ => LifecycleCommand::Crashed { exit_code },
        };

        match self.machine.apply(command.clone()) {
            Ok(zamin_core::supervisor::Transition::CrashReported(report)) => {
                tracing::warn!(server = %self.server_id, exit_code, "server crashed");
                self.publish_crash(report);
            }
            Ok(_) => {
                tracing::info!(server = %self.server_id, exit_code, "server process exited");
                self.publish_transition(from, Some("process-exited".to_owned()));
            }
            Err(_) => {}
        }

        if self.pending_restart
            && self.machine.state() == zamin_core::supervisor::state::ServerState::Stopped
        {
            self.pending_restart = false;
            let _ = self.start().await;
        }
    }

    fn spawn_log_pumps(&mut self) {
        let Some(spawned) = self.spawned.as_mut() else {
            return;
        };
        let child = spawned.handle().child();

        if let Some(stdout) = child.stdout.take() {
            let hub = self.hub.clone();
            let server_id = self.server_id.clone();
            tokio::spawn(pump_logs(stdout, server_id, hub, LogLevel::Unknown));
        }
        if let Some(stderr) = child.stderr.take() {
            let hub = self.hub.clone();
            let server_id = self.server_id.clone();
            tokio::spawn(pump_logs(stderr, server_id, hub, LogLevel::Error));
        }
    }

    fn persist_runtime_record(&self) {
        if let Some(identity) = &self.identity {
            let record = serde_json::json!({
                "schemaVersion": 1,
                "pid": identity.pid,
                "startMarker": identity.start_marker,
                "startedAtMs": self.started_at_ms.unwrap_or(0),
            });
            let path = self.runtime_path();
            let _ = std::fs::create_dir_all(&self.runtime_dir);
            let _ = std::fs::write(path, record.to_string());
        }
    }

    fn clear_runtime_record(&self) {
        let _ = std::fs::remove_file(self.runtime_path());
    }

    fn runtime_path(&self) -> PathBuf {
        self.runtime_dir.join("runtime.json")
    }

    /// Adoption entry point: verify the recorded identity and, only on a
    /// match, claim the server as running (ADR-0005). Foreign identities
    /// are never adopted and never killed.
    pub fn adopt(mut self, record: AdoptRecord) -> mpsc::Sender<ActorCommand> {
        let (tx, rx) = mpsc::channel(64);
        let identity = ProcessIdentity {
            pid: record.pid,
            start_marker: record.start_marker.clone(),
        };
        if platform::process().is_alive(&identity) {
            self.machine.apply(LifecycleCommand::AdoptVerified).ok();
            self.adopted_identity = Some(identity);
            self.started_at_ms = Some(record.started_at_ms);
            tracing::info!(server = %self.server_id, pid = record.pid, "adopted running server");
        } else {
            self.machine.apply(LifecycleCommand::AdoptForeign).ok();
            self.hub.publish_event(
                Some(self.server_id.clone()),
                zamin_protocol::streams::CoreEvent::ServerStateChanged {
                    server_id: self.server_id.clone(),
                    from: ServerState::Adopting,
                    to: ServerState::Unknown,
                    reason: Some("process identity mismatch".to_owned()),
                    exit_code: None,
                    error: None,
                    crash: None,
                },
            );
            tracing::warn!(
                server = %self.server_id,
                pid = record.pid,
                "recorded process identity does not match; surfaced as unknown"
            );
        }
        tokio::spawn(self.run(rx));
        tx
    }
}

pub struct AdoptRecord {
    pub pid: u32,
    pub start_marker: String,
    pub started_at_ms: i64,
}

/// Stream stdout/stderr into per-line batches; the batcher flushes at ~50 ms
/// so IPC carries batches, not per-line messages (ADR-0006).
async fn pump_logs(
    pipe: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    server_id: String,
    hub: HubHandle,
    level: LogLevel,
) {
    let (tx, mut rx) = mpsc::channel::<LogLine>(4096);
    tokio::spawn(async move {
        let mut reader = BufReader::new(pipe);
        let mut buf = Vec::with_capacity(256);
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let raw = String::from_utf8_lossy(&buf);
                    let line = logparse::parse_line(&raw, now_ms());
                    let line = LogLine {
                        level: if level == LogLevel::Unknown {
                            line.level
                        } else {
                            level
                        },
                        ..line
                    };
                    if tx.send(line).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut pending: Vec<LogLine> = Vec::new();
    let mut flush = tokio::time::interval(Duration::from_millis(50));
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            line = rx.recv() => {
                match line {
                    Some(line) => pending.push(line),
                    None => {
                        if !pending.is_empty() {
                            hub.publish_logs(&server_id, std::mem::take(&mut pending));
                        }
                        break;
                    }
                }
            }
            _ = flush.tick() => {
                if !pending.is_empty() {
                    hub.publish_logs(&server_id, std::mem::take(&mut pending));
                }
            }
        }
    }
}

/// Pick the first inspectable candidate. The version requirement table
/// applies once server config carries an MC version; until then any
/// inspectable runtime is acceptable, newest enumeration order first.
fn select_java() -> Result<PathBuf, CoreError> {
    for candidate in zamin_core::java::candidate_paths() {
        if zamin_core::java::inspect(&candidate).is_ok() {
            return Ok(candidate);
        }
    }
    Err(CoreError::JavaNotFound {
        requirement: "any inspectable runtime".to_owned(),
    })
}

pub(crate) fn to_protocol_error(error: &CoreError) -> ProtocolError {
    use zamin_core::error::CoreError as E;
    match error {
        E::InvalidServerId { id, reason } => ProtocolError::new(
            ErrorCode::ServerIdInvalid,
            format!("Server id {id:?} is invalid: {reason}."),
        ),
        E::ServerNotRegistered { id } => ProtocolError::new(
            ErrorCode::ServerNotFound,
            format!("Server {id:?} is not registered."),
        ),
        E::ServerAlreadyRegistered { id } => ProtocolError::new(
            ErrorCode::ServerIdExists,
            format!("Server {id:?} is already registered."),
        ),
        E::ServerRootAlreadyRegistered { path } => ProtocolError::new(
            ErrorCode::ServerIdExists,
            format!("Directory {path:?} is already registered as another server."),
        ),
        E::PathEscapesRoot { path } | E::OutsideRoot { path } => ProtocolError::new(
            ErrorCode::FsPathEscapesRoot,
            format!("Path {path:?} escapes the managed server root."),
        ),
        E::NotFound { path } => ProtocolError::new(
            ErrorCode::FsNotFound,
            format!("Path {path:?} does not exist."),
        ),
        E::NeedsEula { path } => ProtocolError::new(
            ErrorCode::NeedsEula,
            format!("The server cannot start: EULA at {path:?} is missing or not accepted."),
        )
        .with_remediation(&["accept_eula"]),
        E::NotWritable { path } => ProtocolError::new(
            ErrorCode::FsNotWritable,
            format!("The server directory is not writable by the current user: {path:?}."),
        )
        .with_remediation(&["check_permissions"]),
        E::ReadTooLarge { .. } => ProtocolError::new(ErrorCode::InternalError, error.to_string()),
        E::Io { path, source } => ProtocolError::new(
            ErrorCode::InternalError,
            format!("I/O error at {path:?}: {source}."),
        ),
        E::RegistryCorrupt { path, reason } => ProtocolError::new(
            ErrorCode::InternalError,
            format!("State file {path:?} is corrupt: {reason}."),
        ),
        E::SchemaVersion {
            path,
            found,
            expected,
        } => ProtocolError::new(
            ErrorCode::InternalError,
            format!("State file {path:?} has schema version {found}, expected {expected}."),
        ),
        E::JavaInspectFailed { path, reason } => ProtocolError::new(
            ErrorCode::JavaExecFailed,
            format!("Java runtime at {path:?} could not be inspected: {reason}."),
        ),
        E::JavaNotFound { requirement } => ProtocolError::new(
            ErrorCode::JavaNotFound,
            format!("No compatible Java runtime found ({requirement})."),
        )
        .with_remediation(&["install_java", "choose_runtime"]),
        E::PortInUse { port } => ProtocolError::new(
            ErrorCode::PortInUse,
            format!("Port {port} is already in use."),
        )
        .with_context("port", *port as u64)
        .with_remediation(&["choose_another_port", "stop_managed_server"]),
        E::Platform(e) => ProtocolError::new(ErrorCode::InternalError, e.to_string()),
    }
}

fn as_internal(message: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::InternalError, message)
}

fn typed_rejection(rejection: zamin_core::supervisor::state::InvalidTransition) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::ServerNotRunning,
        format!("The request conflicts with the current lifecycle state: {rejection}."),
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
