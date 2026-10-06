//! Human-facing rendering. `--json` bypasses all of this.

use std::fmt::Write as _;

use zamin_cli::ClientError;
use zamin_ipc::{Endpoint, IpcError};
use zamin_protocol::error::ProtocolError;
use zamin_protocol::server::{ServerDetails, ServerState, ServerSummary};
use zamin_protocol::streams::{LogLevel, LogLine};

pub fn state_text(state: ServerState) -> &'static str {
    match state {
        ServerState::NotRunning => "not-running",
        ServerState::Starting => "starting",
        ServerState::Running => "running",
        ServerState::Stopping => "stopping",
        ServerState::Stopped => "stopped",
        ServerState::FailedPreflight => "failed-preflight",
        ServerState::Crashed => "crashed",
        ServerState::Adopting => "adopting",
        ServerState::Unknown => "unknown",
    }
}

pub fn server_table(servers: &[ServerSummary]) {
    if servers.is_empty() {
        println!("No servers registered. Register one with `zamin register <id> <dir>`.");
        return;
    }
    let id_width = servers
        .iter()
        .map(|s| s.server_id.len())
        .max()
        .unwrap_or(4)
        .max(2);
    let name_width = servers
        .iter()
        .map(|s| s.display_name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    println!(
        "{:<id_width$}  {:<name_width$}  STATE",
        "ID",
        "NAME",
        id_width = id_width,
        name_width = name_width
    );
    for server in servers {
        println!(
            "{:<id_width$}  {:<name_width$}  {}",
            server.server_id,
            server.display_name,
            state_text(server.state),
            id_width = id_width,
            name_width = name_width
        );
    }
}

pub fn server_details(details: &ServerDetails) {
    println!("server:  {}", details.server_id);
    println!("name:    {}", details.display_name);
    println!("state:   {}", state_text(details.state));
    if let Some(software) = &details.software {
        println!("software: {software}");
    }
    if let Some(version) = &details.version {
        println!("version: {version}");
    }
    if let Some(port) = details.port {
        println!("port:    {port}");
    }
}

pub fn daemon_status(value: &serde_json::Value) {
    println!(
        "daemon:  {} {} (protocol {})",
        value["daemon"]["name"].as_str().unwrap_or("?"),
        value["daemon"]["version"].as_str().unwrap_or("?"),
        value["protocol"]
    );
    println!(
        "servers: {} registered, {} running",
        value["servers"], value["running"]
    );
}

/// One log line from a raw JSON value (logs.range results).
pub fn log_line(value: &serde_json::Value) {
    let line = value["line"].as_str().unwrap_or("");
    let level = value["level"].as_str().unwrap_or("unknown");
    let thread = value["thread"].as_str();
    print_log(level, thread, line);
}

/// One log line from the typed stream payload.
pub fn log_line_typed(line: &LogLine) {
    let level = match line.level {
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
        LogLevel::Debug => "debug",
        LogLevel::Unknown => "unknown",
    };
    print_log(level, line.thread.as_deref(), &line.line);
}

fn print_log(level: &str, thread: Option<&str>, line: &str) {
    let level = level.to_ascii_uppercase();
    match thread {
        Some(thread) => println!("{level:<6} [{thread}] {line}"),
        None => println!("{level:<6} {line}"),
    }
}

pub fn client_error(error: &ClientError) -> String {
    match error {
        ClientError::Ipc(_) => connection_error(error, &None),
        ClientError::Protocol(protocol) => protocol_error(protocol),
        other => format!("zamin: {other}"),
    }
}

pub fn connection_error(error: &ClientError, endpoint_arg: &Option<String>) -> String {
    let at = match endpoint_arg {
        Some(value) => value.clone(),
        None => endpoint_text(&Endpoint::default_endpoint()),
    };
    let text = match error {
        ClientError::Ipc(IpcError::NoDaemon) => format!(
            "zamin: zamind is not running — nothing is listening at {at}.\n  Start the daemon first (the Panel launches it too), or pass --endpoint."
        ),
        ClientError::Timeout(_) => format!(
            "zamin: the daemon at {at} did not answer in time.\n  It may be wedged; retry, then restart it if this persists."
        ),
        _ => format!("zamin: {error}"),
    };
    text
}

pub fn protocol_error(protocol: &ProtocolError) -> String {
    let mut text = format!("zamin: {protocol}");
    if !protocol.remediation.is_empty() {
        let _ = write!(text, "\n  suggested actions:");
        for action in &protocol.remediation {
            let _ = write!(text, "\n  - {action}");
        }
    }
    text
}

fn endpoint_text(endpoint: &Endpoint) -> String {
    match endpoint {
        Endpoint::WindowsPipe(name) => format!(r"\\.\pipe\{name}"),
        Endpoint::UnixSocket(path) => path.display().to_string(),
    }
}
