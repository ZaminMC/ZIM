//! `zamin` — the command line client (Phase 2, ARCHITECTURE-REVIEW §23):
//! list/status/start/stop/restart/kill/logs/attach over the Zamin
//! Protocol, usable over SSH, with `--json` for scripting. The `plugins`
//! and `jobs` groups reach the catalog surface (ADR-0012) and the daemon's
//! long-running jobs. A second client validating the protocol end to end;
//! it knows the protocol, never the daemon's internals (ADR-0002).

mod render;

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use tokio::io::AsyncBufReadExt as _;

use clap::{Parser, Subcommand};
use zamin_cli::{Client, ClientError};
use zamin_ipc::Endpoint;
use zamin_protocol::jobs::{CancelJobParams, GetJobParams, Job, JobState, ListJobsResult};
use zamin_protocol::methods;
use zamin_protocol::plugins::{
    PluginsDeleteParams, PluginsInstallParams, PluginsInstallResult, PluginsSearchResult,
    PluginsVersionsResult,
};
use zamin_protocol::server::{
    EmptyResult, LifecycleResult, ListServersResult, RegisterServerParams, RegisterServerResult,
    RemoveServerParams, ServerDetails, ServerIdParams, StdinParams, UpdateServerParams,
};
use zamin_protocol::streams::{StreamKind, StreamNotification, StreamPayload};

#[derive(Parser)]
#[command(
    name = "zamin",
    version,
    about = "Manage ZaminPanel servers from the command line",
    after_help = "The daemon (zamind) must be running; zamin connects to it over the local transport."
)]
struct Cli {
    /// Daemon endpoint: a socket path (Unix) or pipe name (Windows).
    /// Defaults to the per-user endpoint zamind listens on.
    #[arg(long, global = true, value_name = "SOCKET_OR_PIPE")]
    endpoint: Option<String>,

    /// Operate a remote box: connect to its agent (host:port) over TLS
    /// instead of the local daemon. Requires --token-file, and
    /// --fingerprint (recommended) or --insecure-skip-verify (explicit).
    #[arg(long, global = true, value_name = "ADDR")]
    remote: Option<String>,

    /// The agent's certificate fingerprint (SHA-256 hex, printed at agent
    /// startup). Pins TLS: the connection proves it talks to THAT agent.
    #[arg(long, global = true, value_name = "HEX")]
    fingerprint: Option<String>,

    /// File holding the agent's token (0600, written at agent bootstrap).
    #[arg(long, global = true, value_name = "PATH")]
    token_file: Option<PathBuf>,

    /// Accept any TLS certificate. Encryption stays on, but no server
    /// identity is proven and the token can then be stolen by a MITM.
    #[arg(long, global = true)]
    insecure_skip_verify: bool,

    /// Machine-readable output: pretty JSON of the raw protocol result.
    /// Errors print the protocol error object and exit 1.
    #[arg(long, global = true)]
    json: bool,

    /// Seconds to wait for a daemon reply before giving up.
    #[arg(long, global = true, value_name = "SECS", default_value_t = 10)]
    timeout: u64,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List every registered server and its lifecycle state
    List,
    /// Show one server's details
    Status { server_id: String },
    /// Show the daemon's own status
    Daemon,
    /// Register an existing server directory with the daemon
    Register {
        server_id: String,
        /// Path to the server's root directory (the one bootstrap path
        /// the protocol accepts; afterwards servers are referenced by id)
        root_path: String,
        /// Display name (defaults to the server id)
        #[arg(long)]
        name: Option<String>,
    },
    /// Rename a server
    Rename {
        server_id: String,
        #[arg(long)]
        name: String,
    },
    /// Remove a server from the registry (the directory is untouched)
    Remove {
        server_id: String,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Start a server
    Start { server_id: String },
    /// Stop a server gracefully
    Stop { server_id: String },
    /// Restart a server
    Restart { server_id: String },
    /// Kill a server (the force path of the stop ladder)
    Kill { server_id: String },
    /// Print a server's recent log lines from logs/latest.log
    Logs {
        server_id: String,
        /// Keep following: recent lines first, then live output until
        /// interrupted
        #[arg(short = 'f', long)]
        follow: bool,
        /// How many trailing lines to print
        #[arg(long, value_name = "N", default_value_t = 100)]
        lines: u32,
        /// Page backward: return lines ending at or before this byte
        /// offset (the previous page's startOffset)
        #[arg(long, value_name = "OFFSET")]
        before: Option<u64>,
    },
    /// Attach to a server console: log lines in, typed lines to stdin
    Attach { server_id: String },
    /// The plugin catalog (ADR-0012): search Modrinth, install and
    /// delete jars; the server's own directory decides where they land
    Plugins {
        #[command(subcommand)]
        command: PluginsCommands,
    },
    /// Server schedules (ADR-0014): the daemon fires timed restarts,
    /// backups, and console lines itself
    Schedules {
        #[command(subcommand)]
        command: SchedulesCommands,
    },
    /// Publish a server (ADR-0017): pick files, review the diff and the
    /// security scan, package, and hand the package to a provider
    Publish {
        #[command(subcommand)]
        command: PublishCommands,
    },
    /// Show or set a server's configuration (ADR-0019, founder §38–39):
    /// memory, JVM args, timeouts, retention — layered over the global
    /// defaults, with provenance shown
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// A server's network picture (founder §37): desired port, the
    /// server.properties authority, a live availability probe, conflicts
    Network {
        #[command(subcommand)]
        command: NetworkCommands,
    },
    /// Long-running daemon jobs: installs, backups, downloads
    Jobs {
        #[command(subcommand)]
        command: JobsCommands,
    },
}

#[derive(Subcommand)]
enum PluginsCommands {
    /// Search the Modrinth catalog for the server's loader family
    Search {
        server_id: String,
        /// Search words; empty lists the most downloaded
        query: Option<String>,
        /// Maximum hits to show
        #[arg(long, value_name = "N", default_value_t = 10)]
        limit: u32,
    },
    /// List installable versions of one project (the pin list)
    Versions {
        server_id: String,
        project_id: String,
    },
    /// List the plugin jars in the server's target directory
    Installed { server_id: String },
    /// Install a plugin: latest for the loader, or a pinned version.
    /// A cancellable job; --wait polls it to the end. Updating a file
    /// that is already installed with different content needs --replace.
    Install {
        server_id: String,
        project_id: String,
        /// Pin an exact version id (from `zamin plugins versions`)
        #[arg(long, value_name = "ID")]
        version: Option<String>,
        /// Poll the job until it finishes, printing byte progress
        #[arg(long)]
        wait: bool,
        /// Overwrite the installed file when its content differs; a
        /// re-install of the identical file never needs this
        #[arg(long)]
        replace: bool,
        /// The update rule's retire step (ADR-0012): the installed file
        /// this install UPGRADES — removed after the new bytes land and
        /// verify. A version bump usually changes the file name, so an
        /// update without --retire leaves both jars on disk.
        #[arg(long, value_name = "FILE")]
        retire: Option<String>,
    },
    /// Delete a plugin jar from the server's target directory
    Delete {
        server_id: String,
        file_name: String,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Check the catalog for updates of the installed jars: the disk's
    /// bytes identify each jar, the catalog answers what it publishes
    Updates { server_id: String },
}

#[derive(Subcommand)]
enum SchedulesCommands {
    /// List the server's schedules and the clock's memory of each
    List { server_id: String },
    /// Add a schedule. The when is one of: --every SECS (a fixed
    /// interval while the daemon runs), --at HH:MM (every day, the
    /// daemon's local clock), or --weekdays mon,wed --at HH:MM. The
    /// then is a restart (default), --backup, or --command "LINE".
    Add {
        server_id: String,
        /// A short name shown in the panel
        #[arg(long)]
        name: String,
        /// Fire every SECONDS while the daemon runs
        #[arg(long, value_name = "SECS", conflicts_with_all = ["at", "weekdays"])]
        every: Option<u64>,
        /// Daily fire time, 24-hour HH:MM (the daemon's local clock)
        #[arg(long, value_name = "HH:MM", conflicts_with = "every")]
        at: Option<String>,
        /// Weekly fire days with --at: mon,tue,wed,thu,fri,sat,sun
        #[arg(long, value_name = "DAYS", requires = "at")]
        weekdays: Option<String>,
        /// Take a backup instead of restarting
        #[arg(long, conflicts_with = "command")]
        backup: bool,
        /// Send this console line instead of restarting
        #[arg(long, value_name = "LINE", conflicts_with = "backup")]
        command: Option<String>,
    },
    /// Remove a schedule (the id comes from `schedules list`)
    Remove {
        server_id: String,
        schedule_id: String,
    },
    /// Pause a schedule: the clock skips it entirely
    Pause {
        server_id: String,
        schedule_id: String,
    },
    /// Resume a paused schedule
    Resume {
        server_id: String,
        schedule_id: String,
    },
}

#[derive(Subcommand)]
enum PublishCommands {
    /// Show the publish configuration as it stands
    Show { server_id: String },
    /// Replace the publish configuration. Unspecified flags keep their
    /// current value. Rules read `folder:PATH`, `file:PATH`, or
    /// `glob:PATTERN` (e.g. --include "glob:plugins/**/*.yml").
    Config {
        server_id: String,
        /// The publish provider id (`zamin publish providers` lists them)
        #[arg(long, value_name = "ID")]
        provider: Option<String>,
        /// The package's title (the server id is used when blank)
        #[arg(long, value_name = "TEXT")]
        title: Option<String>,
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
        #[arg(long, value_name = "VER")]
        version: Option<String>,
        #[arg(long, value_name = "TEXT")]
        changelog: Option<String>,
        /// A provider setting, KEY=VAL (e.g. --setting outDir=/srv/out)
        #[arg(long, value_name = "KEY=VAL")]
        setting: Vec<String>,
        /// Include rules (repeatable; replaces the current list)
        #[arg(long = "include", value_name = "RULE")]
        includes: Vec<String>,
        /// Exclude rules (repeatable; replaces the current list)
        #[arg(long = "exclude", value_name = "RULE")]
        excludes: Vec<String>,
    },
    /// List the available publish providers
    Providers,
    /// Preview the publication: the M/A/D diff and the security scan
    Preview { server_id: String },
    /// Publish: package the selection and hand it to the provider
    Run {
        server_id: String,
        /// Publish anyway despite unreviewed scan findings — the
        /// founder's explicit Publish Anyway confirmation
        #[arg(long)]
        confirm_unsafe: bool,
    },
    /// Review a scan finding as a false positive (--unreview clears it)
    Review {
        server_id: String,
        /// The finding's file (as the preview printed it)
        file: String,
        /// The finding's kind (as the preview printed it)
        kind: String,
        /// Clear the review instead of setting it
        #[arg(long)]
        unreview: bool,
    },
    /// Show the last publication: when, provider, receipt, package
    State { server_id: String },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show the effective settings, where each value comes from, and the
    /// composed start command
    Show { server_id: String },
    /// Set overrides. Flags you omit keep their current value; pass
    /// --clear-<field> to drop an override so the global default applies
    /// again. Changes apply the next time the server starts.
    Set {
        server_id: String,
        /// The server's display name (shows in the panel and listings)
        #[arg(long)]
        display_name: Option<String>,
        /// Server-root relative path to the jar the server boots
        #[arg(long)]
        jar: Option<String>,
        /// Drop the jar override (the built-in server.jar applies)
        #[arg(long)]
        clear_jar: bool,
        /// The Minecraft port players join (1024–65534)
        #[arg(long)]
        port: Option<u16>,
        /// Drop the port override (the global default applies)
        #[arg(long)]
        clear_port: bool,
        /// Minimum heap in MiB (-Xms)
        #[arg(long)]
        min_memory_mb: Option<u32>,
        /// Maximum heap in MiB (-Xmx)
        #[arg(long)]
        max_memory_mb: Option<u32>,
        /// Drop the minimum-heap override
        #[arg(long)]
        clear_min_memory: bool,
        /// Drop the maximum-heap override
        #[arg(long)]
        clear_max_memory: bool,
        /// Path to a specific java executable (else the best managed
        /// runtime is picked)
        #[arg(long)]
        java_path: Option<String>,
        /// Drop the java-path override
        #[arg(long)]
        clear_java_path: bool,
        /// An extra JVM argument; repeat for several (e.g. --jvm-arg
        /// -XX:+UseG1GC)
        #[arg(long)]
        jvm_arg: Vec<String>,
        /// Drop all extra JVM arguments
        #[arg(long)]
        clear_jvm_args: bool,
        /// Graceful stop timeout in seconds
        #[arg(long)]
        stop_timeout_secs: Option<u32>,
        /// Startup validation window in seconds
        #[arg(long)]
        startup_timeout_secs: Option<u32>,
        /// Backup retention: keep the newest N backups
        #[arg(long)]
        backup_keep: Option<u32>,
        /// Drop the backup-retention override
        #[arg(long)]
        clear_backup_keep: bool,
        /// The Minecraft version this server runs (drives the Java
        /// requirement when no direct override is set)
        #[arg(long)]
        mc_version: Option<String>,
        /// Drop the Minecraft-version override
        #[arg(long)]
        clear_mc_version: bool,
        /// Direct override of the required Java major
        #[arg(long)]
        java_major: Option<u32>,
        /// Drop the Java-major override
        #[arg(long)]
        clear_java_major: bool,
    },
}

#[derive(Subcommand)]
enum NetworkCommands {
    /// Probe the server's ports: desired, actual (server.properties),
    /// availability right now, and other servers claiming the same port
    Status { server_id: String },
}

#[derive(Subcommand)]
enum JobsCommands {
    /// List the daemon's recent jobs, running and finished
    List,
    /// Show one job's full state and progress
    Get { job_id: String },
    /// Ask a running job to cancel (it stops at its next check)
    Cancel { job_id: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    #[allow(clippy::expect_used)] // process boundary: no runtime, no CLI
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime builds");

    let json_mode = cli.json;
    match runtime.block_on(run(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            if json_mode {
                // Machine-readable failure: one error object on stdout —
                // the verbatim protocol error when the daemon replied,
                // a synthesized DAEMON_UNREACHABLE object otherwise.
                match &failure.json_error {
                    Some(error) => match serde_json::to_string_pretty(error) {
                        Ok(text) => println!("{text}"),
                        Err(e) => eprintln!("zamin: cannot serialize error: {e}"),
                    },
                    None => eprint!("{}", failure.text),
                }
            } else {
                eprint!("{}", failure.text);
            }
            failure.code
        }
    }
}

/// A command failure: an exit code, the human-facing text, and — when an
/// error object exists — its machine-readable JSON form.
struct Failure {
    code: ExitCode,
    text: String,
    json_error: Option<serde_json::Value>,
}

impl Failure {
    fn new(code: ExitCode, text: String) -> Failure {
        Failure {
            code,
            text,
            json_error: None,
        }
    }

    fn error(text: String) -> Failure {
        Failure::new(ExitCode::from(1), text)
    }

    fn with_json(mut self, error: serde_json::Value) -> Failure {
        self.json_error = Some(error);
        self
    }
}

/// Client-synthesized code for transport-level failures; the daemon was
/// never reached, so no protocol error object exists.
const DAEMON_UNREACHABLE: &str = "DAEMON_UNREACHABLE";

impl From<ClientError> for Failure {
    fn from(error: ClientError) -> Failure {
        let text = render::client_error(&error);
        let json_error = match &error {
            ClientError::Protocol(protocol) => serde_json::to_value(protocol).ok(),
            _ => Some(serde_json::json!({
                "code": DAEMON_UNREACHABLE,
                "message": error.to_string(),
            })),
        };
        let mut failure = Failure::error(text);
        failure.json_error = json_error;
        failure
    }
}

type CmdResult = Result<(), Failure>;

/// Build the remote connection: read the token file, decide trust, connect
/// through the agent. Fingerprint pinning is the default posture; skipping
/// verification is allowed but loudly announced.
async fn connect_remote(cli: &Cli, addr: String) -> Result<Client, Failure> {
    let trust = if let Some(hex) = &cli.fingerprint {
        zamin_agent::client::Trust::Fingerprint(hex.clone())
    } else if cli.insecure_skip_verify {
        eprintln!(
            "WARNING: --insecure-skip-verify is on: the agent's certificate is NOT \
             pinned, so this connection proves no server identity. Use --fingerprint."
        );
        zamin_agent::client::Trust::InsecureSkipVerify
    } else {
        return Err(Failure::error(
            "--remote needs --fingerprint (the hex the agent printed at startup), \
             or an explicit --insecure-skip-verify."
                .to_owned(),
        ));
    };

    let token_path = cli.token_file.clone().ok_or_else(|| {
        Failure::error(
            "--remote needs --token-file (the 0600 token file written at agent bootstrap)"
                .to_owned(),
        )
    })?;
    let token = std::fs::read_to_string(&token_path)
        .map_err(|error| {
            Failure::error(format!(
                "cannot read token file {}: {error}",
                token_path.display()
            ))
        })?
        .trim()
        .to_owned();
    if token.is_empty() {
        return Err(Failure::error(format!(
            "token file {} is empty",
            token_path.display()
        )));
    }

    let cfg = zamin_agent::client::RemoteConnect { addr, token, trust };
    Client::connect_remote(cfg)
        .await
        .map_err(|error| connect_failure(error, &None))
}

/// Connect-time failures get the endpoint-aware human message; protocol
/// errors (a rejected handshake) keep their verbatim error object.
fn connect_failure(error: ClientError, endpoint_arg: &Option<String>) -> Failure {
    let text = render::connection_error(&error, endpoint_arg);
    match &error {
        ClientError::Protocol(_) => error.into(),
        _ => Failure::error(text).with_json(serde_json::json!({
            "code": DAEMON_UNREACHABLE,
            "message": error.to_string(),
        })),
    }
}

async fn run(cli: Cli) -> Result<(), Failure> {
    // Confirmations never need a connection, so they happen first: a
    // mismatched answer ends the run here, a match falls through to the
    // command itself.
    if let Commands::Remove {
        server_id,
        yes: false,
    } = &cli.command
    {
        confirm_removal(server_id)?;
    }
    if let Commands::Plugins {
        command:
            PluginsCommands::Delete {
                server_id: _,
                file_name,
                yes: false,
            },
    } = &cli.command
    {
        confirm_delete(file_name)?;
    }

    let client = match &cli.remote {
        Some(addr) => connect_remote(&cli, addr.clone()).await?,
        None => {
            let endpoint = match &cli.endpoint {
                Some(value) => Endpoint::from_daemon_arg(value),
                None => Endpoint::default_endpoint(),
            };
            Client::connect(endpoint)
                .await
                .map_err(|error| connect_failure(error, &cli.endpoint))?
        }
    };

    match &cli.command {
        Commands::List => list(&cli, &client).await,
        Commands::Status { server_id } => status(&cli, &client, server_id).await,
        Commands::Daemon => daemon(&cli, &client).await,
        Commands::Register {
            server_id,
            root_path,
            name,
        } => {
            register(
                &cli,
                &client,
                server_id,
                root_path,
                name.clone().unwrap_or_else(|| server_id.clone()),
            )
            .await
        }
        Commands::Rename { server_id, name } => rename(&cli, &client, server_id, name).await,
        Commands::Remove { server_id, .. } => remove(&cli, &client, server_id).await,
        Commands::Start { server_id } => {
            lifecycle(&cli, &client, methods::SERVER_START, server_id).await
        }
        Commands::Stop { server_id } => {
            lifecycle(&cli, &client, methods::SERVER_STOP, server_id).await
        }
        Commands::Restart { server_id } => {
            lifecycle(&cli, &client, methods::SERVER_RESTART, server_id).await
        }
        Commands::Kill { server_id } => {
            lifecycle(&cli, &client, methods::SERVER_KILL, server_id).await
        }
        Commands::Logs {
            server_id,
            follow,
            lines,
            before,
        } => {
            if *follow {
                follow_logs(&client, server_id).await
            } else {
                recent_logs(&cli, &client, server_id, *lines, *before).await
            }
        }
        Commands::Attach { server_id } => attach(&client, server_id).await,
        Commands::Plugins { command } => match command {
            PluginsCommands::Search {
                server_id,
                query,
                limit,
            } => {
                plugins_search(
                    &cli,
                    &client,
                    server_id,
                    query.clone().unwrap_or_default(),
                    *limit,
                )
                .await
            }
            PluginsCommands::Versions {
                server_id,
                project_id,
            } => plugins_versions(&cli, &client, server_id, project_id).await,
            PluginsCommands::Installed { server_id } => {
                plugins_installed(&cli, &client, server_id).await
            }
            PluginsCommands::Install {
                server_id,
                project_id,
                version,
                wait,
                replace,
                retire,
            } => {
                plugins_install(
                    &cli,
                    &client,
                    server_id,
                    project_id,
                    InstallOpts {
                        version: version.as_deref(),
                        wait: *wait,
                        replace: *replace,
                        retire: retire.as_deref(),
                    },
                )
                .await
            }
            PluginsCommands::Delete {
                server_id,
                file_name,
                yes: _,
            } => plugins_delete(&cli, &client, server_id, file_name).await,
            PluginsCommands::Updates { server_id } => {
                plugins_updates(&cli, &client, server_id).await
            }
        },
        Commands::Jobs { command } => match command {
            JobsCommands::List => jobs_list(&cli, &client).await,
            JobsCommands::Get { job_id } => jobs_get(&cli, &client, job_id).await,
            JobsCommands::Cancel { job_id } => jobs_cancel(&cli, &client, job_id).await,
        },
        Commands::Schedules { command } => match command {
            SchedulesCommands::List { server_id } => schedules_list(&cli, &client, server_id).await,
            SchedulesCommands::Add {
                server_id,
                name,
                every,
                at,
                weekdays,
                backup,
                command: command_line,
            } => {
                schedules_add(
                    &cli,
                    &client,
                    server_id,
                    name,
                    SchedulesAddOpts {
                        every: *every,
                        at: at.clone(),
                        weekdays: weekdays.clone(),
                        backup: *backup,
                        command: command_line.clone(),
                    },
                )
                .await
            }
            SchedulesCommands::Remove {
                server_id,
                schedule_id,
            } => schedules_remove(&cli, &client, server_id, schedule_id).await,
            SchedulesCommands::Pause {
                server_id,
                schedule_id,
            } => schedules_pause_resume(&cli, &client, server_id, schedule_id, false).await,
            SchedulesCommands::Resume {
                server_id,
                schedule_id,
            } => schedules_pause_resume(&cli, &client, server_id, schedule_id, true).await,
        },
        Commands::Publish { command } => match command {
            PublishCommands::Show { server_id } => {
                publish_config_show(&cli, &client, server_id).await
            }
            PublishCommands::Config {
                server_id,
                provider,
                title,
                description,
                version,
                changelog,
                setting,
                includes,
                excludes,
            } => {
                publish_config_set(
                    &cli,
                    &client,
                    server_id,
                    PublishConfigOpts {
                        provider: provider.as_deref(),
                        title: title.as_deref(),
                        description: description.as_deref(),
                        version: version.as_deref(),
                        changelog: changelog.as_deref(),
                        settings: setting,
                        includes,
                        excludes,
                    },
                )
                .await
            }
            PublishCommands::Providers => publish_providers(&cli, &client).await,
            PublishCommands::Preview { server_id } => {
                publish_preview(&cli, &client, server_id).await
            }
            PublishCommands::Run {
                server_id,
                confirm_unsafe,
            } => publish_run(&cli, &client, server_id, *confirm_unsafe).await,
            PublishCommands::Review {
                server_id,
                file,
                kind,
                unreview,
            } => publish_review(&cli, &client, server_id, file, kind, !*unreview).await,
            PublishCommands::State { server_id } => publish_state(&cli, &client, server_id).await,
        },
        Commands::Config { command } => match command {
            ConfigCommands::Show { server_id } => config_show(&cli, &client, server_id).await,
            ConfigCommands::Set {
                server_id,
                display_name,
                jar,
                clear_jar,
                port,
                clear_port,
                min_memory_mb,
                max_memory_mb,
                clear_min_memory,
                clear_max_memory,
                java_path,
                clear_java_path,
                jvm_arg,
                clear_jvm_args,
                stop_timeout_secs,
                startup_timeout_secs,
                backup_keep,
                clear_backup_keep,
                mc_version,
                clear_mc_version,
                java_major,
                clear_java_major,
            } => {
                config_set(
                    &cli,
                    &client,
                    server_id,
                    ConfigSetOpts {
                        display_name: display_name.as_deref(),
                        jar: jar.as_deref(),
                        clear_jar: *clear_jar,
                        port: *port,
                        clear_port: *clear_port,
                        min_memory_mb: *min_memory_mb,
                        max_memory_mb: *max_memory_mb,
                        clear_min_memory: *clear_min_memory,
                        clear_max_memory: *clear_max_memory,
                        java_path: java_path.as_deref(),
                        clear_java_path: *clear_java_path,
                        jvm_args: jvm_arg.clone(),
                        clear_jvm_args: *clear_jvm_args,
                        stop_timeout_secs: *stop_timeout_secs,
                        startup_timeout_secs: *startup_timeout_secs,
                        backup_keep: *backup_keep,
                        clear_backup_keep: *clear_backup_keep,
                        mc_version: mc_version.as_deref(),
                        clear_mc_version: *clear_mc_version,
                        java_major: *java_major,
                        clear_java_major: *clear_java_major,
                    },
                )
                .await
            }
        },
        Commands::Network { command } => match command {
            NetworkCommands::Status { server_id } => network_status(&cli, &client, server_id).await,
        },
    }
}

fn confirm_removal(server_id: &str) -> Result<(), Failure> {
    eprintln!("Removing '{server_id}' unregisters it (the directory is untouched).");
    eprint!("Type the server id to confirm: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() || line.trim() != server_id {
        return Err(Failure::error(format!(
            "confirmation did not match; '{server_id}' was not removed."
        )));
    }
    Ok(())
}

fn confirm_delete(file_name: &str) -> Result<(), Failure> {
    eprintln!("Deleting '{file_name}' removes the jar from the server's target directory.");
    eprint!("Type the file name to confirm: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() || line.trim() != file_name {
        return Err(Failure::error(format!(
            "confirmation did not match; '{file_name}' was not deleted."
        )));
    }
    Ok(())
}

fn request_id() -> uuid::Uuid {
    uuid::Uuid::now_v7()
}

/// Parses a job id argument; the message teaches the id's shape instead
/// of dumping a UUID error.
fn job_id_or_fail(job_id: &str) -> Result<uuid::Uuid, Failure> {
    uuid::Uuid::parse_str(job_id).map_err(|_| {
        Failure::error(format!(
            "'{job_id}' is not a job id (a UUID, like the ones `zamin jobs list` prints)"
        ))
    })
}

async fn list(cli: &Cli, client: &Client) -> CmdResult {
    let result: ListServersResult = client
        .request_typed(methods::SERVER_LIST, serde_json::json!({}))
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::server_table(&result.servers);
    Ok(())
}

async fn status(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let params = ServerIdParams {
        request_id: request_id(),
        server_id: server_id.to_owned(),
    };
    let details: ServerDetails = client
        .request_typed(methods::SERVER_GET, params)
        .await
        .map_err(protocol_with_usage_hint)?;
    if cli.json {
        print_json(&details);
        return Ok(());
    }
    render::server_details(&details);
    Ok(())
}

async fn daemon(cli: &Cli, client: &Client) -> CmdResult {
    let value = client
        .request(methods::DAEMON_STATUS, serde_json::json!({}))
        .await?;
    if cli.json {
        print_json(&value);
        return Ok(());
    }
    render::daemon_status(&value);
    Ok(())
}

async fn register(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    root_path: &str,
    display_name: String,
) -> CmdResult {
    let params = RegisterServerParams {
        request_id: request_id(),
        server_id: server_id.to_owned(),
        display_name,
        root_path: root_path.to_owned(),
    };
    let result: RegisterServerResult = client
        .request_typed(methods::SERVER_REGISTER, params)
        .await
        .map_err(protocol_with_usage_hint)?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    println!(
        "Registered '{}' ({}) — state {}.",
        result.server.server_id,
        root_path,
        render::state_text(result.server.state)
    );
    Ok(())
}

async fn rename(cli: &Cli, client: &Client, server_id: &str, name: &str) -> CmdResult {
    let params = UpdateServerParams {
        request_id: request_id(),
        server_id: server_id.to_owned(),
        display_name: Some(name.to_owned()),
    };
    let details: ServerDetails = client.request_typed(methods::SERVER_UPDATE, params).await?;
    if cli.json {
        print_json(&details);
        return Ok(());
    }
    println!(
        "Renamed '{}' to '{}'.",
        details.server_id, details.display_name
    );
    Ok(())
}

async fn remove(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let params = RemoveServerParams {
        request_id: request_id(),
        server_id: server_id.to_owned(),
    };
    let _empty: EmptyResult = client.request_typed(methods::SERVER_REMOVE, params).await?;
    if cli.json {
        print_json(&serde_json::json!({"removed": server_id}));
        return Ok(());
    }
    println!("Removed '{server_id}'. Its directory was left untouched.");
    Ok(())
}

/// start / stop / restart / kill share one shape.
async fn lifecycle(cli: &Cli, client: &Client, method: &str, server_id: &str) -> CmdResult {
    let params = ServerIdParams {
        request_id: request_id(),
        server_id: server_id.to_owned(),
    };
    let result: LifecycleResult = client.request_typed(method, params).await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    let verb = method.strip_prefix("server.").unwrap_or(method);
    println!(
        "'{}' {verb} accepted — state {}.",
        result.server_id,
        render::state_text(result.state)
    );
    println!("Outcomes arrive as events; watch with `zamin logs` or `zamin status`.");
    Ok(())
}

async fn recent_logs(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    lines: u32,
    before: Option<u64>,
) -> CmdResult {
    let mut params = serde_json::json!({"serverId": server_id, "maxLines": lines});
    if let Some(offset) = before {
        params["beforeOffset"] = serde_json::json!(offset);
    }
    let value = client.request(methods::LOGS_RANGE, params).await?;
    if cli.json {
        print_json(&value);
        return Ok(());
    }
    let older = value["olderAvailable"].as_bool().unwrap_or(false);
    if let Some(lines) = value["lines"].as_array() {
        for line in lines {
            render::log_line(line);
        }
    }
    if older {
        let cursor = value["startOffset"].as_u64().unwrap_or(0);
        eprintln!("(older lines exist; page back with --before {cursor})");
    }
    Ok(())
}

/// `logs -f`: recent buffer, then live, until interrupted.
async fn follow_logs(client: &Client, server_id: &str) -> CmdResult {
    let mut receiver = client
        .subscribe(StreamKind::Logs, Some(server_id.to_owned()))
        .await?;
    loop {
        tokio::select! {
            note = receiver.recv() => match note {
                Ok(notification) => print_notification(&notification),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("--- {n} log line(s) missed (output too fast); catch up with `zamin logs {server_id}` ---");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err(Failure::error("the daemon closed the connection".to_owned()));
                }
            },
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}

/// `attach`: the logs stream comes in, typed lines go out as `server.stdin`.
/// Local-only commands: /quit or /detach leave the console.
async fn attach(client: &Client, server_id: &str) -> CmdResult {
    let mut receiver = client
        .subscribe(StreamKind::Logs, Some(server_id.to_owned()))
        .await?;
    println!("Attached to '{server_id}'. Server commands go to its stdin; /quit detaches.");
    println!("Ctrl-C also detaches (the server keeps running).");

    let printer = tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(notification) => print_notification(&notification),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("--- {n} log line(s) missed ---");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            line = stdin.next_line() => match line {
                Ok(Some(line)) => {
                    let trimmed = line.trim();
                    if trimmed == "/quit" || trimmed == "/detach" {
                        break;
                    }
                    if trimmed.is_empty() {
                        continue;
                    }
                    let params = StdinParams {
                        request_id: request_id(),
                        server_id: server_id.to_owned(),
                        line: line.trim_end_matches(['\r', '\n']).to_owned(),
                    };
                    // A stopped server must not kill the attachment: the
                    // operator may be reading the tail while deciding.
                    if let Err(error) = client
                        .request_typed::<StdinParams, EmptyResult>(
                            methods::SERVER_STDIN, params)
                        .await
                    {
                        eprintln!("{}", render::client_error(&error));
                    }
                }
                Ok(None) => break, // EOF (Ctrl-D)
                Err(e) => {
                    return Err(Failure::error(format!("cannot read stdin: {e}")));
                }
            },
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    printer.abort();
    Ok(())
}

fn print_notification(notification: &StreamNotification) {
    match &notification.payload {
        StreamPayload::Logs { batch } => {
            for line in batch {
                render::log_line_typed(line);
            }
        }
        StreamPayload::Missed { missed } => {
            eprintln!("--- {missed} notification(s) missed while slow ---");
        }
        StreamPayload::Event { .. } | StreamPayload::Metrics { .. } => {}
    }
}

// --- plugins (ADR-0012): the catalog over the wire ---

async fn plugins_search(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    query: String,
    limit: u32,
) -> CmdResult {
    let params = zamin_protocol::plugins::PluginsSearchParams {
        server_id: server_id.to_owned(),
        query,
        limit: Some(limit),
    };
    let result: PluginsSearchResult = client
        .request_typed(methods::PLUGINS_SEARCH, params)
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::plugin_hits(&result);
    Ok(())
}

async fn plugins_versions(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    project_id: &str,
) -> CmdResult {
    let params = zamin_protocol::plugins::PluginsVersionsParams {
        server_id: server_id.to_owned(),
        project_id: project_id.to_owned(),
    };
    let result: PluginsVersionsResult = client
        .request_typed(methods::PLUGINS_VERSIONS, params)
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::plugin_versions(&result);
    Ok(())
}

async fn plugins_installed(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let params = zamin_protocol::plugins::PluginsInstalledParams {
        server_id: server_id.to_owned(),
    };
    let result: zamin_protocol::plugins::PluginsInstalledResult = client
        .request_typed(methods::PLUGINS_INSTALLED, params)
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::installed_plugins(&result);
    Ok(())
}

async fn plugins_updates(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let params = zamin_protocol::plugins::PluginsUpdatesParams {
        server_id: server_id.to_owned(),
    };
    let result: zamin_protocol::plugins::PluginsUpdatesResult = client
        .request_typed(methods::PLUGINS_UPDATES, params)
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::plugin_updates(&result);
    Ok(())
}

/// The install command's knobs, bundled so the handler stays under
/// clippy's argument cap and the call site reads like the recipe.
struct InstallOpts<'a> {
    version: Option<&'a str>,
    wait: bool,
    replace: bool,
    retire: Option<&'a str>,
}

async fn plugins_install(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    project_id: &str,
    opts: InstallOpts<'_>,
) -> CmdResult {
    let InstallOpts {
        version,
        wait,
        replace,
        retire,
    } = opts;
    let params = PluginsInstallParams {
        server_id: server_id.to_owned(),
        project_id: project_id.to_owned(),
        version_id: version.map(str::to_owned),
        replace,
        retire_file: retire.map(str::to_owned),
    };
    let result: PluginsInstallResult = client
        .request_typed(methods::PLUGINS_INSTALL, params)
        .await?;

    if !wait {
        if cli.json {
            print_json(&result);
            return Ok(());
        }
        let job = &result.job;
        println!(
            "Install queued as job {} — watch with `zamin jobs get {}`.",
            job.job_id, job.job_id
        );
        println!("Or re-run with --wait to follow the byte progress here.");
        return Ok(());
    }

    // --wait: poll the job to a terminal state. In JSON mode only the
    // finished job prints — one object, script-friendly.
    let finished = wait_for_job(client, job_id_of(&result.job)).await?;
    if cli.json {
        print_json(&finished);
        return Ok(());
    }
    println!(
        "Install {} — {}.",
        match finished.state {
            JobState::Succeeded => "finished",
            _ => "did not finish",
        },
        render::job_state_text(finished.state)
    );
    if finished.state != JobState::Succeeded {
        return Err(Failure::error(format!(
            "install job ended {}{}",
            render::job_state_text(finished.state),
            finished
                .error
                .as_ref()
                .map(|e| format!(": {e}"))
                .unwrap_or_default()
        )));
    }
    Ok(())
}

fn job_id_of(job: &Job) -> uuid::Uuid {
    job.job_id
}

/// Polls `jobs.get` until the job reaches a terminal state, printing each
/// change (state or byte progress) as it happens. The last observed job
/// is returned so callers can report the ending honestly.
async fn wait_for_job(client: &Client, job_id: uuid::Uuid) -> Result<Job, Failure> {
    let mut last_mark: Option<(JobState, u64)> = None;
    loop {
        let job: Job = client
            .request_typed(methods::JOBS_GET, GetJobParams { job_id })
            .await?;
        let progress_current = job.progress.as_ref().map(|p| p.current).unwrap_or(0);
        let mark = (job.state, progress_current);
        if last_mark != Some(mark) {
            eprintln!("{}", render::job_progress_line(&job));
        }
        last_mark = Some(mark);
        match job.state {
            JobState::Queued | JobState::Running => {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
            JobState::Succeeded | JobState::Failed | JobState::Cancelled => return Ok(job),
        }
    }
}

async fn plugins_delete(cli: &Cli, client: &Client, server_id: &str, file_name: &str) -> CmdResult {
    let params = PluginsDeleteParams {
        server_id: server_id.to_owned(),
        file_name: file_name.to_owned(),
    };
    let _empty: EmptyResult = client
        .request_typed(methods::PLUGINS_DELETE, params)
        .await?;
    if cli.json {
        print_json(&serde_json::json!({"deleted": file_name}));
        return Ok(());
    }
    println!("Deleted '{file_name}'.");
    Ok(())
}

// --- jobs: the daemon's long-running operations, inspected ---

async fn jobs_list(cli: &Cli, client: &Client) -> CmdResult {
    let result: ListJobsResult = client
        .request_typed(methods::JOBS_LIST, serde_json::json!({}))
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::job_table(&result.jobs);
    Ok(())
}

async fn jobs_get(cli: &Cli, client: &Client, job_id: &str) -> CmdResult {
    let job_id = job_id_or_fail(job_id)?;
    let job: Job = client
        .request_typed(methods::JOBS_GET, GetJobParams { job_id })
        .await?;
    if cli.json {
        print_json(&job);
        return Ok(());
    }
    render::job_details(&job);
    Ok(())
}

async fn jobs_cancel(cli: &Cli, client: &Client, job_id: &str) -> CmdResult {
    let job_id = job_id_or_fail(job_id)?;
    let params = CancelJobParams {
        request_id: request_id(),
        job_id,
    };
    let job: Job = client.request_typed(methods::JOBS_CANCEL, params).await?;
    if cli.json {
        print_json(&job);
        return Ok(());
    }
    println!(
        "Cancel asked for job {} ({}); it stops at its next check.",
        job.job_id,
        render::job_state_text(job.state)
    );
    Ok(())
}

fn print_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(e) => eprintln!("zamin: cannot serialize output: {e}"),
    }
}

/// Wraps protocol errors with the `zamin list` hint where it helps.
fn protocol_with_usage_hint(error: ClientError) -> Failure {
    let is_unknown_server = matches!(
        &error,
        ClientError::Protocol(protocol)
            if protocol.code == zamin_protocol::error::ErrorCode::ServerNotFound
    );
    let mut failure: Failure = error.into();
    if is_unknown_server {
        failure
            .text
            .push_str("\n  (list registered servers with `zamin list`)");
    }
    failure
}

// --- schedules (ADR-0014) ------------------------------------------------

/// The add command's knobs, bundled like InstallOpts so the call site
/// reads like the operator's sentence.
struct SchedulesAddOpts {
    every: Option<u64>,
    at: Option<String>,
    weekdays: Option<String>,
    backup: bool,
    command: Option<String>,
}

async fn schedules_list(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let params = zamin_protocol::schedules::SchedulesListParams {
        server_id: server_id.to_owned(),
    };
    let result: zamin_protocol::schedules::SchedulesListResult = client
        .request_typed(methods::SCHEDULES_LIST, params)
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::schedule_table(&result);
    Ok(())
}

async fn schedules_add(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    name: &str,
    opts: SchedulesAddOpts,
) -> CmdResult {
    let SchedulesAddOpts {
        every,
        at,
        weekdays,
        backup,
        command,
    } = opts;

    // The when: exactly one shape. clap refuses --every with --at, but a
    // missing when is a usage error this message explains.
    let spec = if let Some(secs) = every {
        zamin_protocol::schedules::ScheduleSpec::Interval { every_secs: secs }
    } else if let Some(at) = &at {
        let weekdays = match weekdays {
            Some(days) => {
                let list: Vec<String> = days
                    .split(',')
                    .map(|day| day.trim().to_ascii_lowercase())
                    .filter(|day| !day.is_empty())
                    .collect();
                if list.is_empty() {
                    return Err(Failure::error(
                        "--weekdays needs at least one day (mon..sun)".to_owned(),
                    ));
                }
                list
            }
            None => Vec::new(),
        };
        if weekdays.is_empty() {
            zamin_protocol::schedules::ScheduleSpec::Daily { at: at.clone() }
        } else {
            zamin_protocol::schedules::ScheduleSpec::Weekly {
                weekdays,
                at: at.clone(),
            }
        }
    } else {
        return Err(Failure::error(
            "pick a when: --every SECS, --at HH:MM (daily), or --weekdays DAYS --at HH:MM"
                .to_owned(),
        ));
    };

    // The then: restart (default), backup, or a console line.
    let action = if let Some(line) = command {
        zamin_protocol::schedules::ScheduleAction::Command { line }
    } else if backup {
        zamin_protocol::schedules::ScheduleAction::Backup
    } else {
        zamin_protocol::schedules::ScheduleAction::Restart
    };

    let params = zamin_protocol::schedules::SchedulesCreateParams {
        server_id: server_id.to_owned(),
        name: name.to_owned(),
        spec,
        action,
        enabled: true,
    };
    let result: zamin_protocol::schedules::SchedulesCreateResult = client
        .request_typed(methods::SCHEDULES_CREATE, params)
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    println!(
        "Schedule {:?} added ({}) — the daemon fires it from its own clock.",
        result.schedule.schedule.name,
        render::schedule_spec_text(&result.schedule.schedule.spec),
    );
    println!(
        "Pause or remove it any time: `zamin schedules pause/remove {server_id} {}`.",
        result.schedule.schedule.id
    );
    Ok(())
}

async fn schedules_remove(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    schedule_id: &str,
) -> CmdResult {
    let params = zamin_protocol::schedules::SchedulesDeleteParams {
        server_id: server_id.to_owned(),
        schedule_id: schedule_id.to_owned(),
    };
    let _: zamin_protocol::server::EmptyResult = client
        .request_typed(methods::SCHEDULES_DELETE, params)
        .await?;
    if cli.json {
        print_json(&serde_json::json!({ "removed": schedule_id }));
        return Ok(());
    }
    println!("Schedule {schedule_id} removed.");
    Ok(())
}

async fn schedules_pause_resume(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    schedule_id: &str,
    enabled: bool,
) -> CmdResult {
    let params = zamin_protocol::schedules::SchedulesUpdateParams {
        server_id: server_id.to_owned(),
        schedule_id: schedule_id.to_owned(),
        name: None,
        spec: None,
        action: None,
        enabled: Some(enabled),
    };
    let result: zamin_protocol::schedules::SchedulesUpdateResult = client
        .request_typed(methods::SCHEDULES_UPDATE, params)
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    println!(
        "Schedule {:?} is now {}.",
        result.schedule.schedule.name,
        if enabled { "resumed" } else { "paused" },
    );
    Ok(())
}

// ---- Publish (founder §40–47, §74, ADR-0017) ------------------------

/// Parse one CLI selection rule: `folder:PATH`, `file:PATH`, or
/// `glob:PATTERN`. The daemon re-validates; this split just needs to
/// catch typos early with a message that teaches the syntax.
fn parse_rule(raw: &str) -> Result<zamin_protocol::publish::SelectionRule, Failure> {
    let (kind, payload) = raw.split_once(':').ok_or_else(|| {
        Failure::error(format!(
            "a publish rule reads `folder:PATH`, `file:PATH`, or `glob:PATTERN`; got {raw:?}"
        ))
    })?;
    match kind.trim().to_ascii_lowercase().as_str() {
        "folder" => Ok(zamin_protocol::publish::SelectionRule::Folder {
            path: payload.to_owned(),
        }),
        "file" => Ok(zamin_protocol::publish::SelectionRule::File {
            path: payload.to_owned(),
        }),
        "glob" => Ok(zamin_protocol::publish::SelectionRule::Glob {
            pattern: payload.to_owned(),
        }),
        other => Err(Failure::error(format!(
            "unknown rule kind {other:?} — use folder:, file:, or glob:"
        ))),
    }
}

async fn publish_config_show(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let params = zamin_protocol::publish::PublishConfigGetParams {
        server_id: server_id.to_owned(),
    };
    let config: zamin_protocol::publish::PublishConfig = client
        .request_typed(methods::PUBLISH_CONFIG_GET, params)
        .await?;
    if cli.json {
        print_json(&config);
        return Ok(());
    }
    render::publish_config(&config);
    Ok(())
}

/// The `publish config` overrides: every field the operator did not pass
/// keeps its current value (the daemon replace is full; the CLI merges).
struct PublishConfigOpts<'a> {
    provider: Option<&'a str>,
    title: Option<&'a str>,
    description: Option<&'a str>,
    version: Option<&'a str>,
    changelog: Option<&'a str>,
    settings: &'a [String],
    includes: &'a [String],
    excludes: &'a [String],
}

async fn publish_config_set(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    opts: PublishConfigOpts<'_>,
) -> CmdResult {
    let PublishConfigOpts {
        provider,
        title,
        description,
        version,
        changelog,
        settings,
        includes,
        excludes,
    } = opts;
    // config.set is a full replace; the CLI fetches the current config
    // and applies only the flags the operator passed, so a partial
    // command never silently erases the rest.
    let current: zamin_protocol::publish::PublishConfig = client
        .request_typed(
            methods::PUBLISH_CONFIG_GET,
            zamin_protocol::publish::PublishConfigGetParams {
                server_id: server_id.to_owned(),
            },
        )
        .await?;

    let mut settings_map = current.provider_settings.clone();
    for pair in settings {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(Failure::error(format!(
                "a provider setting reads KEY=VAL; got {pair:?}"
            )));
        };
        settings_map.insert(key.trim().to_owned(), value.to_owned());
    }

    let config = zamin_protocol::publish::PublishConfig {
        selection: zamin_protocol::publish::PublishSelection {
            includes: includes
                .iter()
                .map(|r| parse_rule(r))
                .collect::<Result<Vec<_>, _>>()?,
            excludes: excludes
                .iter()
                .map(|r| parse_rule(r))
                .collect::<Result<Vec<_>, _>>()?,
        },
        provider_id: provider.map(str::to_owned).unwrap_or(current.provider_id),
        provider_settings: settings_map,
        title: title.map(str::to_owned).unwrap_or(current.title),
        description: description
            .map(str::to_owned)
            .unwrap_or(current.description),
        version: version.map(str::to_owned).unwrap_or(current.version),
        changelog: changelog.map(str::to_owned).unwrap_or(current.changelog),
    };
    let saved: zamin_protocol::publish::PublishConfig = client
        .request_typed(
            methods::PUBLISH_CONFIG_SET,
            zamin_protocol::publish::PublishConfigSetParams {
                server_id: server_id.to_owned(),
                config: config.clone(),
            },
        )
        .await?;
    if cli.json {
        print_json(&saved);
        return Ok(());
    }
    println!("Publish configuration saved.");
    render::publish_config(&saved);
    Ok(())
}

async fn publish_providers(cli: &Cli, client: &Client) -> CmdResult {
    let result: zamin_protocol::publish::ProvidersListResult = client
        .request_typed(
            methods::PUBLISH_PROVIDERS_LIST,
            zamin_protocol::publish::ProvidersListParams {},
        )
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::providers_table(&result);
    Ok(())
}

async fn publish_preview(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let result: zamin_protocol::publish::PublishPreviewResult = client
        .request_typed(
            methods::PUBLISH_PREVIEW,
            zamin_protocol::publish::PublishPreviewParams {
                server_id: server_id.to_owned(),
            },
        )
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::publish_preview(&result);
    Ok(())
}

async fn publish_run(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    confirm_unsafe: bool,
) -> CmdResult {
    let result: zamin_protocol::publish::PublishExecuteResult = client
        .request_typed(
            methods::PUBLISH_EXECUTE,
            zamin_protocol::publish::PublishExecuteParams {
                server_id: server_id.to_owned(),
                confirm_unsafe: Some(confirm_unsafe),
            },
        )
        .await?;
    let job = result.job;
    if cli.json {
        print_json(&job);
        return Ok(());
    }
    println!(
        "Publishing as job {} — packaging, then {}.",
        job.job_id,
        if confirm_unsafe {
            "publishing DESPITE unreviewed findings"
        } else {
            "uploading"
        },
    );
    let (state, error) = wait_publish_job(client, &job.job_id.to_string()).await?;
    if state != "succeeded" {
        let detail = error
            .map(|e| e.to_string())
            .unwrap_or_else(|| "no details".to_owned());
        return Err(Failure::error(format!("the publish job {state}: {detail}")));
    }
    println!("Published.");
    Ok(())
}

/// Poll jobs.get until the publish job reaches a terminal state.
async fn wait_publish_job(
    client: &Client,
    job_id: &str,
) -> Result<(String, Option<serde_json::Value>), Failure> {
    loop {
        let job: serde_json::Value = client
            .request(methods::JOBS_GET, serde_json::json!({ "jobId": job_id }))
            .await?;
        let state = job["state"].as_str().unwrap_or_default().to_owned();
        if state != "running" && state != "queued" {
            return Ok((
                state,
                job["error"].as_object().map(|_| job["error"].clone()),
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

async fn publish_review(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    file: &str,
    kind: &str,
    reviewed: bool,
) -> CmdResult {
    let result: zamin_protocol::publish::PublishPreviewResult = client
        .request_typed(
            methods::PUBLISH_REVIEW_SET,
            zamin_protocol::publish::PublishReviewSetParams {
                server_id: server_id.to_owned(),
                file: file.to_owned(),
                kind: kind.to_owned(),
                reviewed,
            },
        )
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    println!(
        "Review {} for {file:?}.",
        if reviewed { "recorded" } else { "cleared" },
    );
    render::publish_findings(&result.scan);
    println!(
        "\n{} finding(s) still block an execute (zamin publish preview {} shows everything).",
        result.blocking_count, server_id
    );
    Ok(())
}

async fn publish_state(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let result: zamin_protocol::publish::PublishStateResult = client
        .request_typed(
            methods::PUBLISH_STATE,
            zamin_protocol::publish::PublishStateParams {
                server_id: server_id.to_owned(),
            },
        )
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::publish_state(&result);
    Ok(())
}

// --- config & network (founder §37–39, ADR-0019) ------------------------

/// The `config set` overrides: flags the operator passed become tri-state
/// patch entries; flags omitted keep their current value; `--clear-<field>`
/// drops the override so the global default applies again.
struct ConfigSetOpts<'a> {
    display_name: Option<&'a str>,
    jar: Option<&'a str>,
    clear_jar: bool,
    port: Option<u16>,
    clear_port: bool,
    min_memory_mb: Option<u32>,
    max_memory_mb: Option<u32>,
    clear_min_memory: bool,
    clear_max_memory: bool,
    java_path: Option<&'a str>,
    clear_java_path: bool,
    jvm_args: Vec<String>,
    clear_jvm_args: bool,
    stop_timeout_secs: Option<u32>,
    startup_timeout_secs: Option<u32>,
    backup_keep: Option<u32>,
    clear_backup_keep: bool,
    mc_version: Option<&'a str>,
    clear_mc_version: bool,
    java_major: Option<u32>,
    clear_java_major: bool,
}

async fn config_show(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let result: zamin_protocol::config::ConfigGetResult = client
        .request_typed(
            methods::CONFIG_GET,
            zamin_protocol::config::ConfigGetParams {
                server_id: server_id.to_owned(),
            },
        )
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::config_view(&result);
    Ok(())
}

async fn config_set(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    opts: ConfigSetOpts<'_>,
) -> CmdResult {
    let mut settings = zamin_protocol::config::ServerSettingsPatch::default();
    if let Some(port) = opts.port {
        settings.port = Some(Some(port));
    }
    if opts.clear_port {
        settings.port = Some(None);
    }
    if let Some(v) = opts.min_memory_mb {
        settings.min_memory_mb = Some(Some(v));
    }
    if opts.clear_min_memory {
        settings.min_memory_mb = Some(None);
    }
    if let Some(v) = opts.max_memory_mb {
        settings.max_memory_mb = Some(Some(v));
    }
    if opts.clear_max_memory {
        settings.max_memory_mb = Some(None);
    }
    if let Some(v) = opts.java_path {
        settings.java_path = Some(Some(v.to_owned()));
    }
    if opts.clear_java_path {
        settings.java_path = Some(None);
    }
    if !opts.jvm_args.is_empty() {
        settings.extra_jvm_args = Some(Some(opts.jvm_args.clone()));
    }
    if opts.clear_jvm_args {
        settings.extra_jvm_args = Some(None);
    }
    if let Some(v) = opts.stop_timeout_secs {
        settings.stop_timeout_secs = Some(Some(v));
    }
    if let Some(v) = opts.startup_timeout_secs {
        settings.startup_timeout_secs = Some(Some(v));
    }
    if let Some(v) = opts.backup_keep {
        settings.backup_keep = Some(Some(v));
    }
    if opts.clear_backup_keep {
        settings.backup_keep = Some(None);
    }
    if let Some(v) = opts.mc_version {
        settings.mc_version = Some(Some(v.to_owned()));
    }
    if opts.clear_mc_version {
        settings.mc_version = Some(None);
    }
    if let Some(v) = opts.java_major {
        settings.java_major_required = Some(Some(v));
    }
    if opts.clear_java_major {
        settings.java_major_required = Some(None);
    }

    let jar = if opts.clear_jar {
        Some(None)
    } else {
        opts.jar.map(|j| Some(j.to_owned()))
    };

    if settings.is_empty() && jar.is_none() && opts.display_name.is_none() {
        return Err(Failure::error(
            "nothing to set: pass at least one flag (--port, --max-memory-mb, --jvm-arg,              --display-name, ...) or a --clear-<field> to drop an override."
                .to_owned(),
        ));
    }

    let params = zamin_protocol::config::ConfigSetParams {
        server_id: server_id.to_owned(),
        display_name: opts.display_name.map(str::to_owned),
        jar,
        settings,
    };
    let result: zamin_protocol::config::ConfigGetResult =
        client.request_typed(methods::CONFIG_SET, params).await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    println!("Configuration saved. Overrides apply the next time the server starts.");
    render::config_view(&result);
    Ok(())
}

async fn network_status(cli: &Cli, client: &Client, server_id: &str) -> CmdResult {
    let result: zamin_protocol::config::NetworkStatusResult = client
        .request_typed(
            methods::NETWORK_STATUS,
            zamin_protocol::config::NetworkStatusParams {
                server_id: server_id.to_owned(),
            },
        )
        .await?;
    if cli.json {
        print_json(&result);
        return Ok(());
    }
    render::network_status(&result);
    Ok(())
}
