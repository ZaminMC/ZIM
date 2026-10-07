//! The per-server actor (ADR-0005): the sole owner of one server's process,
//! stdin, state machine, and log pipeline. Commands arrive over a bounded
//! channel; everything mutating happens here, serialized — double-start
//! races and interleaved stdin are structurally impossible.

use std::path::{Path, PathBuf};
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
use zamin_protocol::streams::{LogLevel, LogLine, MetricsSample};

use crate::hub::HubHandle;

const TICK: Duration = Duration::from_millis(250);
const GRACEFUL_GRACE: Duration = Duration::from_secs(10);
/// Metrics sampling interval (ADR-0006: 1 Hz per running server). The
/// sampler's own cost is two small /proc reads (or one syscall pair on
/// Windows) per second — far under the < 1% core budget.
const METRICS_INTERVAL: Duration = Duration::from_secs(1);

/// Log lines attached to a crash classification as evidence (§53 crash card).
const CRASH_EVIDENCE_LINES: usize = 3;

/// Disk-headroom sanity floor for a start attempt: 1 GiB. A "sanity"
/// check, not a sizing tool — a fresh server needs this much for the jar,
/// libraries, and first world files; more is always better.
const MIN_DISK_HEADROOM_BYTES: u64 = 1024 * 1024 * 1024;

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
    /// The daemon's managed JDK root (`<data>/java`): auto-selection
    /// considers fetched runtimes alongside system-wide candidates.
    managed_java_root: PathBuf,
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

    /// Set once per startup attempt when the configured startup timeout is
    /// exceeded, so the "still starting" progress notice fires exactly
    /// once (ADR-0005: remain STARTING, surface progress, never guess).
    startup_timeout_surfaced: bool,

    /// Metrics sampling state (ADR-0006): the previous cumulative CPU
    /// counter + wall instant, so a percent can be computed from deltas;
    /// and the last sample instant for the 1 Hz throttle. Reset whenever
    /// no process is live — a percent must never span process generations.
    metrics_cpu: Option<(Duration, Instant)>,
    metrics_last_sample: Instant,
}

impl Actor {
    pub fn new(
        server_id: String,
        root: PathBuf,
        runtime_dir: PathBuf,
        global_config_path: PathBuf,
        managed_java_root: PathBuf,
        hub: HubHandle,
        initial_state: zamin_core::supervisor::state::ServerState,
    ) -> Actor {
        Actor {
            server_id,
            root,
            runtime_dir,
            global_config_path,
            managed_java_root,
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
            startup_timeout_surfaced: false,
            metrics_cpu: None,
            metrics_last_sample: Instant::now(),
        }
    }

    pub fn spawn_task(
        server_id: String,
        root: PathBuf,
        runtime_dir: PathBuf,
        global_config_path: PathBuf,
        managed_java_root: PathBuf,
        hub: HubHandle,
        initial_state: zamin_core::supervisor::state::ServerState,
    ) -> mpsc::Sender<ActorCommand> {
        let (tx, rx) = mpsc::channel(64);
        let actor = Actor::new(
            server_id,
            root,
            runtime_dir,
            global_config_path,
            managed_java_root,
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
                // Adopted servers are supervised too: removing one must
                // terminate its process, never orphan a live JVM (M1).
                if self.spawned.is_some() || self.adopted_identity.is_some() {
                    let _ = self.stop(true).await;
                    // The kill is synchronous; the actor exits after the
                    // retire reply acknowledges the request.
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

        let required = settings
            .java_major_required
            .or_else(|| settings.mc_version.as_deref().and_then(java_major_for));

        let java = settings
            .java_path
            .clone()
            .map(Ok)
            .unwrap_or_else(|| select_java(&self.managed_java_root, required))
            .map_err(|e| self.preflight_failure(e))?;
        let java_info = tokio::task::spawn_blocking({
            let java = java.clone();
            move || zamin_core::java::inspect(&java)
        })
        .await
        .map_err(|e| as_internal(e.to_string()))?
        .map_err(|e| self.preflight_failure(e))?;

        // ADR-0005 preflight: the selected runtime must satisfy the
        // required Java major — declared directly, or derived from the
        // configured Minecraft version. An explicit javaPath is
        // authoritative: an incompatible pick is a typed failure, never
        // a silent substitution.
        if let Some(required) = required {
            if !java_info.satisfies(required) {
                return Err(self.preflight_failure(CoreError::JavaIncompatible {
                    found: java_info.major,
                    required,
                }));
            }
        }

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
        self.startup_timeout_surfaced = false;
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
        let load_path = path.clone();
        match tokio::task::spawn_blocking(move || config::load_server(&load_path)).await {
            Ok(Ok(file)) => file,
            // A corrupt or unreadable per-server file must be loud (the
            // registry/config policy): surface exactly what was ignored,
            // then run with defaults so the daemon stays usable.
            Ok(Err(error)) => {
                tracing::warn!(
                    server = %self.server_id,
                    "per-server config {path:?} unreadable ({error}); using defaults"
                );
                ServerConfigFile {
                    schema_version: config::CONFIG_SCHEMA_VERSION,
                    ..ServerConfigFile::default()
                }
            }
            Err(e) => {
                tracing::warn!(
                    server = %self.server_id,
                    "per-server config load panicked: {e}; using defaults"
                );
                ServerConfigFile {
                    schema_version: config::CONFIG_SCHEMA_VERSION,
                    ..ServerConfigFile::default()
                }
            }
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

        // Disk headroom sanity (ADR-0005): starting a server onto a full
        // disk produces the ugliest mid-world-generation crashes; require
        // a modest floor before spawn.
        let free = {
            let path = self.root.clone();
            match tokio::task::spawn_blocking(move || platform::process().fs_free_bytes(&path))
                .await
            {
                Ok(Ok(free)) => free,
                Ok(Err(source)) => {
                    return Err(CoreError::Io {
                        path: self.root.clone(),
                        source: match source {
                            zamin_core::error::PlatformError::Io(io) => io,
                            other => std::io::Error::other(other.to_string()),
                        },
                    });
                }
                Err(e) => {
                    return Err(CoreError::Io {
                        path: self.root.clone(),
                        source: std::io::Error::other(e.to_string()),
                    });
                }
            }
        };
        if free < MIN_DISK_HEADROOM_BYTES {
            return Err(CoreError::InsufficientDisk {
                path: self.root.clone(),
                available_mb: free / (1024 * 1024),
                required_mb: MIN_DISK_HEADROOM_BYTES / (1024 * 1024),
            });
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
        let state = self.machine.state();
        let adopted = self.spawned.is_none() && self.adopted_identity.is_some();

        // Neither spawned nor adopted: nothing to stop (Stopping stays
        // idempotent-ok while the shutdown ladder runs).
        if self.spawned.is_none() && !adopted {
            return match state {
                zamin_core::supervisor::state::ServerState::Stopping => Ok(self.wire_state()),
                _ => Err(ProtocolError::new(
                    ErrorCode::ServerNotRunning,
                    format!("Server '{}' is not running.", self.server_id),
                )),
            };
        }

        if force {
            // Kill = stop without grace. Move to stopping FIRST so the
            // exit is classified as a deliberate stop, never a crash; then
            // terminate immediately.
            if matches!(
                state,
                zamin_core::supervisor::state::ServerState::Running
                    | zamin_core::supervisor::state::ServerState::Starting
            ) {
                let from = self.wire_state();
                self.machine
                    .apply(LifecycleCommand::Stop)
                    .map_err(typed_rejection)?;
                self.publish_transition(from, Some("kill-requested".to_owned()));
            }
            if let Some(spawned) = self.spawned.as_mut() {
                let _ = spawned.handle().force_kill_tree();
            } else {
                self.kill_verified_adopted();
            }
            return Ok(self.wire_state());
        }

        match state {
            zamin_core::supervisor::state::ServerState::Running
            | zamin_core::supervisor::state::ServerState::Starting => {
                let from = self.wire_state();
                self.machine
                    .apply(LifecycleCommand::Stop)
                    .map_err(typed_rejection)?;
                self.publish_transition(from, Some("stop-requested".to_owned()));
                self.stop_requested_at = Some(Instant::now());
                self.graceful_signaled = false;
                if adopted {
                    // An adopted server has no stdin and no console: the
                    // graceful mechanism is the OS signal (ladder step 3),
                    // issued now. The ladder then waits one grace window
                    // before step 4, matching the spawned ladder's tail.
                    self.signal_adopted_graceful().await;
                } else if let Some(stdin) = self.stdin.as_mut() {
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

    /// Graceful signal for an adopted process (no stdin). The ladder's
    /// stdin step is marked as consumed so force lands one grace window
    /// after the signal. Failures are logged, never hidden; the ladder
    /// proceeds to force regardless (ADR-0005).
    async fn signal_adopted_graceful(&mut self) {
        self.graceful_signaled = true;
        let stop_timeout = Duration::from_secs(
            self.load_effective_settings()
                .await
                .ok()
                .map(|s| s.stop_timeout_secs.max(1) as u64)
                .unwrap_or(60),
        );
        self.stop_requested_at = Some(Instant::now() - stop_timeout);
        if let Some(identity) = &self.adopted_identity {
            match platform::process().signal_graceful(identity.pid) {
                Ok(()) => {
                    tracing::info!(server = %self.server_id, "sent os-level graceful signal to adopted server")
                }
                Err(e) => tracing::warn!(
                    server = %self.server_id,
                    "os-level graceful signal unavailable ({e}); ladder falls through to force"
                ),
            }
        }
    }

    /// Kill an adopted process whose identity was verified at adoption and
    /// is re-verified immediately before the kill — no code path may kill
    /// a process whose identity was not verified (ADR-0005).
    fn kill_verified_adopted(&mut self) {
        if let Some(identity) = &self.adopted_identity {
            if platform::process().is_alive(identity) {
                if let Err(e) = platform::process().force_kill(identity.pid) {
                    tracing::warn!(server = %self.server_id, "force kill failed: {e}");
                }
            }
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
                self.stop_requested_at = None;
                self.graceful_signaled = false;
                tracing::info!(server = %self.server_id, "adopted server exited");
                let from = self.wire_state();
                // A stop we requested completes gracefully; anything else
                // is an unexpected exit of a supervised process.
                let command = match self.machine.state() {
                    zamin_core::supervisor::state::ServerState::Stopping => {
                        LifecycleCommand::StoppedGracefully
                    }
                    _ => LifecycleCommand::Crashed { exit_code: 0 },
                };
                if self.machine.apply(command).is_ok() {
                    self.publish_transition(from, Some("adopted-process-exited".to_owned()));
                }
                if self.pending_restart
                    && self.machine.state() == zamin_core::supervisor::state::ServerState::Stopped
                {
                    self.pending_restart = false;
                    let _ = self.start().await;
                }
            }
        }
        self.sample_metrics();
    }

    /// The 1 Hz metrics sampler (ADR-0006): CPU% from counter deltas, RSS,
    /// live player count, and uptime — published on the hub while a server
    /// process is live, never faked. Runs inline in the tick loop: the read
    /// is a couple of file reads (microseconds), not a blocking syscall.
    fn sample_metrics(&mut self) {
        let live_pid = self
            .spawned
            .as_ref()
            .map(|s| s.pid())
            .or_else(|| self.adopted_identity.as_ref().map(|i| i.pid));
        let Some(pid) = live_pid else {
            // Nothing live: drop the CPU base so the next start computes its
            // first percent from its own generation, never a stale one.
            self.metrics_cpu = None;
            return;
        };
        if self.metrics_last_sample.elapsed() < METRICS_INTERVAL {
            return;
        }
        self.metrics_last_sample = Instant::now();
        let Some(raw) = platform::process().sample_process(pid) else {
            // The process died between the exit poll and this read; the
            // next tick's exit handling owns the announcement.
            return;
        };
        let now = Instant::now();
        let cpu_percent = self.metrics_cpu.and_then(|(prev_cpu, prev_at)| {
            let wall = now.duration_since(prev_at).as_secs_f64();
            let used = raw.cpu_time.checked_sub(prev_cpu)?.as_secs_f64();
            (wall > 0.0).then_some((used / wall) * 100.0)
        });
        self.metrics_cpu = Some((raw.cpu_time, now));
        let sample = MetricsSample {
            ts_ms: now_ms(),
            cpu_percent,
            rss_bytes: raw.rss_bytes,
            players: self.hub.roster_len(&self.server_id).map(|n| n as u32),
            // TPS is only ever set when actually measured (ADR-0006); the
            // daemon does not guess.
            tps: None,
            uptime_ms: self.started_at_ms.map(|start| now_ms() - start),
        };
        self.hub.publish_metrics(&self.server_id, sample);
    }

    async fn check_startup_validation(&mut self) {
        // The WHOLE ring, not a tail window: a server that floods stdout
        // right after its Done line pushes that line past any fixed tail
        // within milliseconds (30k lines/s evicts 5000 in ~166 ms). The
        // ring is bounded; scanning it costs nothing next to the tick.
        let validated = self
            .hub
            .log_ring(&self.server_id)
            .iter()
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
            return;
        }

        // ADR-0005: a startup that exceeds the configured timeout remains
        // STARTING — never guess — but the wait is surfaced exactly once
        // so clients can show progress instead of silence.
        if !self.startup_timeout_surfaced {
            let started_at = self.started_at_ms.unwrap_or(now_ms());
            let elapsed_ms = (now_ms().saturating_sub(started_at)).max(0) as u64;
            let timeout_secs = self
                .load_effective_settings()
                .await
                .ok()
                .map(|s| s.startup_timeout_secs.max(1) as u64)
                .unwrap_or(120);
            if elapsed_ms >= timeout_secs * 1000 {
                self.startup_timeout_surfaced = true;
                tracing::warn!(
                    server = %self.server_id,
                    "startup exceeds configured timeout; still starting (state unchanged per ADR-0005)"
                );
                self.publish_state(Some("startup-timeout-exceeded"), None);
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
            } else {
                // Adopted server: kill by re-verified identity.
                self.kill_verified_adopted();
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
            Ok(zamin_core::supervisor::Transition::CrashReported(mut report)) => {
                tracing::warn!(server = %self.server_id, exit_code, "server crashed");
                // The crash card is the operator's first screen (§53): attach
                // the last ingested log lines as evidence. The reader task
                // usually has the dying process's final lines by the time
                // waitpid returns; if the race bites, the excerpt shows the
                // preceding activity, which is still context worth showing.
                if report.evidence.is_none() {
                    let tail = self.hub.log_ring(&self.server_id);
                    let excerpt: Vec<String> = tail
                        .iter()
                        .rev()
                        .take(CRASH_EVIDENCE_LINES)
                        .rev()
                        .map(|line| line.line.clone())
                        .collect();
                    if !excerpt.is_empty() {
                        report.evidence = Some(excerpt.join("\n"));
                    }
                }
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

            // A dead recorded process is a stale record, not a mystery: the
            // state machine's Reset returns it to not-running so the server
            // can be started again. A LIVE pid whose identity mismatches is
            // someone else's process — it stays unknown and untouchable
            // (ADR-0005: never adopt, never kill a foreign pid).
            if !platform::process().pid_exists(record.pid) {
                self.machine.apply(LifecycleCommand::Reset).ok();
                self.clear_runtime_record();
                self.hub.publish_event(
                    Some(self.server_id.clone()),
                    zamin_protocol::streams::CoreEvent::ServerStateChanged {
                        server_id: self.server_id.clone(),
                        from: ServerState::Unknown,
                        to: ServerState::NotRunning,
                        reason: Some("recorded process is gone".to_owned()),
                        exit_code: None,
                        error: None,
                        crash: None,
                    },
                );
                tracing::info!(
                    server = %self.server_id,
                    pid = record.pid,
                    "recorded process no longer exists; reset to not-running"
                );
            }
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
    // The flush tick is the stream batcher: under load each window carries
    // hundreds of lines, so the webview channel never sees one message per
    // line. The window size is bounded below by the terminal-echo budget
    // (PERFORMANCE-BUDGETS.md): echo latency is quantized by this tick, so
    // at 50 ms the p99 would be ~50 ms + ε — the budget broken by design.
    // 10 ms keeps echo p99 ≈ 10 ms on reference hardware while the flood
    // batches stay huge (asserted by the perf suite's batch-shape check).
    //
    // The line cap is the memory half of the same bargain: a slow
    // subscriber's backlog is bounded per NOTIFICATION, so if a
    // notification could grow with ingestion rate (a whole tick's worth
    // of lines), the backlog would grow with the flood too — measured at
    // 182 MiB under an unbounded flood before the cap existed. 256 lines
    // ≈ 18 KB makes each frame's size rate-independent, so the bounded
    // subscriber queue (1024 frames) is bounded in bytes no matter how
    // fast the server screams. Quiet servers never hit the cap; the tick
    // flushes them.
    const LOG_FLUSH_MAX_LINES: usize = 256;
    let mut flush = tokio::time::interval(Duration::from_millis(10));
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            line = rx.recv() => {
                match line {
                    Some(line) => {
                        pending.push(line);
                        if pending.len() >= LOG_FLUSH_MAX_LINES {
                            hub.publish_logs(&server_id, std::mem::take(&mut pending));
                        }
                    }
                    None => {
                        if !pending.is_empty() {
                            hub.publish_logs(&server_id, std::mem::take(&mut pending));
                        }
                        // The stdout pipe closed: the process is gone, and
                        // with it everyone on it (ADR-0005 — the next boot
                        // starts an empty room).
                        hub.clear_roster(&server_id);
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

/// Pick the first inspectable candidate satisfying `required` (system
/// candidates first, then the daemon's managed runtimes). With no
/// requirement, the first inspectable runtime wins — the historical
/// behavior. A requirement nobody satisfies is a typed JavaNotFound
/// (or JavaIncompatible when candidates exist but are all too old),
/// which the UI turns into its install affordance.
fn select_java(managed_root: &Path, required: Option<u32>) -> Result<PathBuf, CoreError> {
    let mut candidates = zamin_core::java::candidate_paths();
    candidates.extend(zamin_core::java::managed_candidates(managed_root));
    let mut all_too_old: Option<(u32, u32)> = None;
    for candidate in candidates {
        if let Ok(info) = zamin_core::java::inspect(&candidate) {
            match required {
                Some(required) if !info.satisfies(required) => {
                    all_too_old = Some((info.major, required));
                }
                _ => return Ok(candidate),
            }
        }
    }
    Err(match all_too_old {
        Some((found, required)) => CoreError::JavaIncompatible { found, required },
        None => CoreError::JavaNotFound {
            requirement: required
                .map(|r| format!("java major {r}"))
                .unwrap_or_else(|| "any inspectable runtime".to_owned()),
        },
    })
}

/// Required Java major for a configured Minecraft version; `None` when no
/// version is configured or the table does not know it (unknown future
/// versioning must not block a start, ADR-0005).
fn java_major_for(mc_version: &str) -> Option<u32> {
    zamin_core::java::required_major(mc_version)
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
        E::JavaIncompatible { found, required } => ProtocolError::new(
            ErrorCode::JavaIncompatible,
            format!(
                "The selected Java runtime reports major {found}, but this server requires major {required}."
            ),
        )
        .with_context("found", *found as u64)
        .with_context("required", *required as u64)
        .with_remediation(&["install_java", "choose_runtime"]),
        E::InsufficientDisk {
            path,
            available_mb,
            required_mb,
        } => ProtocolError::new(
            ErrorCode::DiskFull,
            format!(
                "Only {available_mb} MiB free at {path:?}; at least {required_mb} MiB is required to start a server."
            ),
        )
        .with_remediation(&["free_space"]),
        E::PortInUse { port } => ProtocolError::new(
            ErrorCode::PortInUse,
            format!("Port {port} is already in use."),
        )
        .with_context("port", *port as u64)
        .with_remediation(&["choose_another_port", "stop_managed_server"]),
        E::ArchiveUnsafeEntry { entry, reason } => ProtocolError::new(
            ErrorCode::ArchiveUnsafeEntry,
            format!("Archive entry {entry:?} is unsafe: {reason}."),
        ),
        E::ArchiveTooLarge {
            found,
            entries,
            max_bytes,
            max_entries,
        } => ProtocolError::new(
            ErrorCode::ArchiveUnsafeEntry,
            format!(
                "The archive exceeds the safety limits: {found} bytes across {entries} entries; \
                 the limits are {max_bytes} bytes and {max_entries} entries."
            ),
        ),
        E::DiskFull { path } => ProtocolError::new(
            ErrorCode::DiskFull,
            format!("The disk is full at {path:?}. Free space and try again."),
        )
        .with_remediation(&["free_space"]),
        E::Cancelled => ProtocolError::new(
            ErrorCode::InternalError,
            "The operation was cancelled before it finished.",
        ),
        E::Http { url, status, reason } => {
            // 404 from a catalog endpoint means "that thing does not
            // exist"; every other HTTP failure (and transport failure
            // below) means the catalog itself is unreachable.
            if *status == 404 {
                ProtocolError::new(
                    ErrorCode::CatalogNotFound,
                    format!("The catalog has nothing at {url} (HTTP 404: {reason})."),
                )
            } else {
                ProtocolError::new(
                    ErrorCode::CatalogUnavailable,
                    format!("The software catalog answered HTTP {status} for {url}: {reason}."),
                )
                .with_remediation(&["retry_later"])
            }
        }
        E::HttpTransport { url, message } => ProtocolError::new(
            ErrorCode::CatalogUnavailable,
            format!("The software catalog is unreachable ({url}): {message}."),
        )
        .with_remediation(&["check_connection", "retry_later"]),
        E::PluginExists { file } => ProtocolError::new(
            ErrorCode::PluginExists,
            format!(
                "The file {file:?} is already installed with different content; \
                 an update must replace it explicitly."
            ),
        )
        .with_context("file", file.clone()),
        E::InvalidSchedule { reason } => ProtocolError::new(
            ErrorCode::ScheduleInvalid,
            format!("The schedule is invalid: {reason}."),
        ),
        E::SchedulesCorrupt { path, reason } => ProtocolError::new(
            ErrorCode::InternalError,
            format!("The schedule store at {path:?} is corrupt: {reason}."),
        ),
        E::InvalidPublishConfig { reason } => ProtocolError::new(
            ErrorCode::ProtocolInvalidRequest,
            format!("The publish configuration is invalid: {reason}."),
        ),
        E::PublishStateCorrupt { path, reason } => ProtocolError::new(
            ErrorCode::InternalError,
            format!("The publish state at {path:?} is corrupt: {reason}."),
        ),
        E::PublishTooLarge {
            found,
            entries,
            max_bytes,
            max_entries,
        } => ProtocolError::new(
            ErrorCode::PublishSelectionTooLarge,
            format!(
                "The publish selection exceeds the safety limits: {found} across {entries} entries; \
                 the limits are {max_bytes} bytes and {max_entries} entries."
            ),
        ),
        E::PublishUpload { provider, reason } => ProtocolError::new(
            ErrorCode::PublishUploadFailed,
            format!("The {provider} upload failed: {reason}."),
        )
        .with_remediation(&["retry_later", "check_provider_settings"]),
        E::ChecksumMismatch {
            path,
            algorithm,
            expected,
            actual,
        } => ProtocolError::new(
            ErrorCode::ChecksumMismatch,
            format!(
                "The downloaded file at {path:?} does not match its published checksum \
                 (expected {algorithm} {expected}, computed {actual}); the download was discarded."
            ),
        )
        .with_context("expected", expected.clone())
        .with_context("actual", actual.clone())
        .with_remediation(&["retry_download"]),
        E::RestoreRolledBack { reason } => ProtocolError::new(
            ErrorCode::InternalError,
            format!("Restore failed mid-commit ({reason}); the previous files were rolled back."),
        ),
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
