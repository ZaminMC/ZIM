//! Human-facing rendering. `--json` bypasses all of this.

use std::fmt::Write as _;

use zamin_cli::ClientError;
use zamin_ipc::{Endpoint, IpcError};
use zamin_protocol::error::ProtocolError;
use zamin_protocol::jobs::{Job, JobKind, JobState};
use zamin_protocol::plugins::{
    PluginUpdateStatus, PluginsInstalledResult, PluginsSearchResult, PluginsUpdatesResult,
    PluginsVersionsResult,
};
use zamin_protocol::schedules::{ScheduleAction, ScheduleSpec, SchedulesListResult};
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
        ClientError::Ipc(IpcError::DaemonBusy) => format!(
            "zamin: the daemon at {at} is out of connection slots (it is alive but saturated).\n  Retry in a moment; if this persists, restart the daemon."
        ),
        ClientError::Timeout(_) => format!(
            "zamin: the daemon at {at} did not answer in time.\n  It may be wedged; retry, then restart it if this persists."
        ),
        ClientError::Remote(detail) => format!(
            "zamin: could not reach the agent at {at}.\n  {detail}\n  Check the agent is running there, the fingerprint matches what it printed at startup, and the token file is current."
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

// --- plugins (ADR-0012) ---

pub fn plugin_hits(result: &PluginsSearchResult) {
    if result.hits.is_empty() {
        println!("No hits. The catalog answers for this server's loader family only.");
        return;
    }
    println!(
        "Installs land in the server's `{}` directory.\n",
        result.target
    );
    let slug_width = width_of(result.hits.iter().map(|h| h.slug.as_str())).max(5);
    let title_width = width_of(result.hits.iter().map(|h| h.title.as_str())).max(5);
    println!(
        "{:<slug_width$}  {:<title_width$}  {:>10}  LOADERS  PROJECT ID",
        "SLUG",
        "TITLE",
        "DOWNLOADS",
        slug_width = slug_width,
        title_width = title_width,
    );
    for hit in &result.hits {
        println!(
            "{:<slug_width$}  {:<title_width$}  {:>10}  {:<7}  {}",
            hit.slug,
            hit.title,
            hit.downloads,
            hit.loaders.join(","),
            hit.project_id,
            slug_width = slug_width,
            title_width = title_width,
        );
    }
    println!("\nInstall with `zamin plugins install <server> <project-id>`.");
}

pub fn plugin_versions(result: &PluginsVersionsResult) {
    if result.versions.is_empty() {
        println!("No installable versions: the project ships nothing for this loader.");
        return;
    }
    println!(
        "Versions for {} (installs land in `{}`):\n",
        result.versions.len(),
        result.target
    );
    let number_width = width_of(result.versions.iter().map(|v| v.version_number.as_str())).max(7);
    println!(
        "{:<number_width$}  {:<8}  GAME VERSIONS  FILE",
        "VERSION",
        "ID",
        number_width = number_width,
    );
    for version in &result.versions {
        println!(
            "{:<number_width$}  {:<8}  {:<13}  {}",
            version.version_number,
            version.id,
            version.game_versions.join(","),
            version.file_name.as_deref().unwrap_or("(not installable)"),
            number_width = number_width,
        );
    }
    println!("\nPin one with `zamin plugins install <server> <project-id> --version <id>`.");
}

pub fn installed_plugins(result: &PluginsInstalledResult) {
    if result.entries.is_empty() {
        println!(
            "No plugin jars in `{}` yet. Install one with `zamin plugins search <server>`.",
            result.target
        );
        return;
    }
    println!("The server's `{}` directory:\n", result.target);
    let name_width = width_of(result.entries.iter().map(|e| e.file_name.as_str())).max(4);
    println!(
        "{:<name_width$}  {:>9}  MODIFIED",
        "FILE",
        "SIZE",
        name_width = name_width
    );
    for entry in &result.entries {
        let note = if entry.symlink_outside {
            "  (symlink out of the server root — not deletable here)"
        } else {
            ""
        };
        println!(
            "{:<name_width$}  {:>9}  {}{}",
            entry.file_name,
            bytes_text(entry.size_bytes),
            utc_date_text(entry.modified_ms),
            note,
            name_width = name_width,
        );
    }
    println!("\nDelete with `zamin plugins delete <server> <file-name>`.");
}

fn update_status_text(status: &PluginUpdateStatus) -> &'static str {
    match status {
        PluginUpdateStatus::UpToDate => "up to date",
        PluginUpdateStatus::UpdateAvailable => "UPDATE AVAILABLE",
        PluginUpdateStatus::Unmanaged => "unmanaged",
    }
}

pub fn plugin_updates(result: &PluginsUpdatesResult) {
    if result.entries.is_empty() {
        println!("No plugin jars in `{}` — nothing to check.", result.target);
        return;
    }
    println!(
        "Update check for the server's `{}` directory:\n",
        result.target
    );
    let name_width = width_of(result.entries.iter().map(|e| e.file_name.as_str())).max(4);
    println!(
        "{:<name_width$}  {:<16}  {:<9}  {:<9}",
        "FILE",
        "STATUS",
        "INSTALLED",
        "LATEST",
        name_width = name_width
    );
    let mut applicable = 0;
    for entry in &result.entries {
        let installed = entry.installed_version.as_deref().unwrap_or("-");
        let latest = entry.latest_version.as_deref().unwrap_or("-");
        if matches!(entry.status, PluginUpdateStatus::UpdateAvailable) {
            applicable += 1;
        }
        println!(
            "{:<name_width$}  {:<16}  {:<9}  {:<9}",
            entry.file_name,
            update_status_text(&entry.status),
            installed,
            latest,
            name_width = name_width,
        );
    }
    if applicable > 0 {
        println!(
            "\nApply one with `zamin plugins install <server> <project-id> \
             --version <latest-id> --replace --retire <file> --wait`\n\
             (the FILE column names the jar; --retire removes it once the \
             new bytes land, so the update does not leave both versions)."
        );
    }
    if result
        .entries
        .iter()
        .any(|e| matches!(e.status, PluginUpdateStatus::Unmanaged))
    {
        println!(
            "Unmanaged jars' bytes are not the catalog's — reinstall them from the \
             catalog (or delete them by hand) to bring them under the update rule."
        );
    }
}

// --- jobs ---

pub fn job_state_text(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Succeeded => "succeeded",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
    }
}

pub fn kind_text(kind: JobKind) -> &'static str {
    match kind {
        JobKind::ServerCreate => "server.create",
        JobKind::BackupCreate => "backup.create",
        JobKind::BackupRestore => "backup.restore",
        JobKind::ArchiveExtract => "archive.extract",
        JobKind::JavaInstall => "java.install",
        JobKind::PluginInstall => "plugins.install",
    }
}

/// A one-line live view of a job, for `install --wait` polling.
pub fn job_progress_line(job: &Job) -> String {
    let state = job_state_text(job.state);
    match &job.progress {
        Some(progress) => match (progress.total, progress.unit.as_deref()) {
            (Some(total), Some(unit)) => {
                format!("  [{}] {}/{} {unit}", state, progress.current, total)
            }
            (Some(total), None) => format!(
                "  [{}] {}/{} ({:.0}%)",
                state,
                progress.current,
                total,
                if total == 0 {
                    0.0
                } else {
                    progress.current as f64 * 100.0 / total as f64
                }
            ),
            (None, _) => format!("  [{}] {}", state, progress.current),
        },
        None => format!("  [{state}]"),
    }
}

pub fn job_table(jobs: &[Job]) {
    if jobs.is_empty() {
        println!("No jobs yet. Installs, backups and downloads appear here.");
        return;
    }
    println!(
        "{:<38}  {:<16}  {:<8}  {:<10}  PROGRESS",
        "JOB", "KIND", "SERVER", "STATE"
    );
    for job in jobs {
        let server = job.server_id.as_deref().unwrap_or("-");
        println!(
            "{:<38}  {:<16}  {:<8}  {:<10}  {}",
            job.job_id,
            kind_text(job.kind),
            server,
            job_state_text(job.state),
            job_progress_line(job).trim_start(),
        );
    }
}

pub fn job_details(job: &Job) {
    println!("job:      {}", job.job_id);
    println!("kind:     {}", kind_text(job.kind));
    if let Some(server) = &job.server_id {
        println!("server:   {server}");
    }
    println!("state:    {}", job_state_text(job.state));
    if let Some(progress) = &job.progress {
        print!("progress: {}", job_progress_line(job).trim_start());
        if let Some(message) = &progress.message {
            print!(" — {message}");
        }
        println!();
    }
    if let Some(error) = &job.error {
        println!("error:    {error}");
    }
    println!("created:  {}", utc_date_text(job.created_at_ms));
    if let Some(started) = job.started_at_ms {
        println!("started:  {}", utc_date_text(started));
    }
    if let Some(ended) = job.ended_at_ms {
        println!("ended:    {}", utc_date_text(ended));
    }
}

/// Human byte size: bytes stay exact, larger units take one decimal.
pub fn bytes_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Epoch milliseconds as a UTC date, no time-zone theater: the daemon's
/// timestamps are instants, the panel renders local time nicely.
pub fn utc_date_text(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let secs_of_day = ms.rem_euclid(86_400_000) / 1000;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Howard Hinnant's civil_from_days: days since 1970-01-01 to a
/// (year, month, day) triple in the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

fn width_of<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    values.map(|value| value.len()).max().unwrap_or(0)
}

/// One line per schedule: the when in words, the action, the clock's
/// memory. The when reads like the operator wrote it ("daily at 04:30"),
/// not like the JSON it is stored as.
pub fn schedule_spec_text(spec: &ScheduleSpec) -> String {
    match spec {
        ScheduleSpec::Interval { every_secs } => {
            let secs = *every_secs;
            if secs % 3600 == 0 {
                format!("every {} h", secs / 3600)
            } else if secs % 60 == 0 {
                format!("every {} min", secs / 60)
            } else {
                format!("every {secs} s")
            }
        }
        ScheduleSpec::Daily { at } => format!("daily at {at}"),
        ScheduleSpec::Weekly { weekdays, at } => {
            format!("weekly ({}) at {at}", weekdays.join(","))
        }
    }
}

fn schedule_action_text(action: &ScheduleAction) -> String {
    match action {
        ScheduleAction::Restart => "restart".to_owned(),
        ScheduleAction::Backup => "backup".to_owned(),
        ScheduleAction::Command { line } => format!("console: {line}"),
    }
}

pub fn schedule_table(result: &SchedulesListResult) {
    if result.schedules.is_empty() {
        println!(
            "No schedules for {} — the daemon runs the clock, but nobody has\n\
             asked it for anything yet (zamin schedules add).",
            result.server_id
        );
        return;
    }
    let name_width = width_of(result.schedules.iter().map(|s| s.schedule.name.as_str())).max(4);
    println!(
        "Schedules for {} (times are the daemon's own clock):\n",
        result.server_id
    );
    println!(
        "{:<name_width$}  {:<24}  {:<20}  CLOCK",
        "NAME",
        "WHEN",
        "THEN",
        name_width = name_width
    );
    for schedule in &result.schedules {
        let status = if !schedule.schedule.enabled {
            "paused".to_owned()
        } else {
            match schedule.schedule.last_fired_ms {
                Some(fired) => format!("fired {}", utc_date_text(fired)),
                None => "never fired".to_owned(),
            }
        };
        println!(
            "{:<name_width$}  {:<24}  {:<20}  {}",
            schedule.schedule.name,
            schedule_spec_text(&schedule.schedule.spec),
            schedule_action_text(&schedule.schedule.action),
            status,
            name_width = name_width
        );
        if schedule.schedule.enabled {
            if let Some(next) = schedule.next_run_ms {
                println!(
                    "{:<name_width$}  next run {}",
                    "",
                    utc_date_text(next),
                    name_width = name_width
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_sizes_stay_human() {
        assert_eq!(bytes_text(0), "0 B");
        assert_eq!(bytes_text(512), "512 B");
        assert_eq!(bytes_text(2048), "2.0 KB");
        assert_eq!(bytes_text(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(bytes_text(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn utc_dates_render_from_epoch_millis() {
        assert_eq!(utc_date_text(0), "1970-01-01 00:00:00 UTC");
        // 2026-09-01T10:00:00Z — the ADR-0012 mock's published date.
        assert_eq!(utc_date_text(1_788_256_800_000), "2026-09-01 10:00:00 UTC");
        assert_eq!(utc_date_text(1_788_256_800_123), "2026-09-01 10:00:00 UTC");
    }

    #[test]
    fn civil_dates_cross_leap_years() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // leap year boundary
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }
}
