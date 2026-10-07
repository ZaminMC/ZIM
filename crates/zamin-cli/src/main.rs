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
            } => {
                plugins_install(
                    &cli,
                    &client,
                    server_id,
                    project_id,
                    version.as_deref(),
                    *wait,
                    *replace,
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

async fn plugins_install(
    cli: &Cli,
    client: &Client,
    server_id: &str,
    project_id: &str,
    version: Option<&str>,
    wait: bool,
    replace: bool,
) -> CmdResult {
    let params = PluginsInstallParams {
        server_id: server_id.to_owned(),
        project_id: project_id.to_owned(),
        version_id: version.map(str::to_owned),
        replace,
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
