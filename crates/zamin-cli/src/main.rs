//! `zamin` — the command line client (Phase 2, ARCHITECTURE-REVIEW §23):
//! list/status/start/stop/restart/kill/logs/attach over the Zamin
//! Protocol, usable over SSH, with `--json` for scripting. A second client
//! validating the protocol end to end; it knows the protocol, never the
//! daemon's internals (ADR-0002).

mod render;

use std::io::Write as _;
use std::process::ExitCode;
use tokio::io::AsyncBufReadExt as _;

use clap::{Parser, Subcommand};
use zamin_cli::{Client, ClientError};
use zamin_ipc::Endpoint;
use zamin_protocol::methods;
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
    // Commands that never need a connection.
    if let Commands::Remove {
        server_id,
        yes: false,
    } = &cli.command
    {
        return confirm_removal(server_id);
    }

    let endpoint = match &cli.endpoint {
        Some(value) => Endpoint::from_daemon_arg(value),
        None => Endpoint::default_endpoint(),
    };

    let client = Client::connect(endpoint)
        .await
        .map_err(|error| connect_failure(error, &cli.endpoint))?;

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

fn request_id() -> uuid::Uuid {
    uuid::Uuid::now_v7()
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
