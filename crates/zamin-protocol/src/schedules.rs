//! Server schedules (ADR-0014): named, timed tasks the daemon itself
//! fires — restarts, backups, console commands. The daemon runs the
//! clock; clients only author the rules and read the results. The
//! schedule file lives beside the server's own daemon metadata and the
//! disk record is the only state — no shadow timers survive a restart
//! unaccounted for (lastFiredMs IS the schedule's memory).

use serde::{Deserialize, Serialize};

/// A stored schedule. `serverId` is not on the record — the file it lives
/// in is the binding — but every method result echoes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub name: String,
    pub spec: ScheduleSpec,
    pub action: ScheduleAction,
    pub enabled: bool,
    pub created_ms: i64,
    /// The last dispatch that actually happened (the action was sent to
    /// the engine), regardless of the action's own outcome. Absent until
    /// the first fire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_ms: Option<i64>,
}

/// When a schedule fires, in the daemon's local time. Wire shapes are
/// plain and self-describing; validation lives in `zamin-core::schedules`
/// so the daemon and every future client can refuse garbage identically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScheduleSpec {
    /// Every `everySecs` seconds while the daemon runs. Re-anchored at
    /// daemon start: downtime never stacks up firings.
    Interval { every_secs: u64 },
    /// Every day at `at` (local daemon time, "HH:MM").
    Daily { at: String },
    /// On the listed weekdays (["mon".."sun"], at least one) at `at`.
    Weekly { weekdays: Vec<String>, at: String },
}

/// What firing does. The daemon dispatches each through its ordinary
/// paths — the same lifecycle verb, the same backup job, the same stdin —
/// so events, jobs, and audit look exactly like an operator's action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScheduleAction {
    /// Restart the server — but only if it is running. A stopped server
    /// stays stopped; a schedule never switches a machine on.
    Restart,
    /// Take a backup; runs whether the server is up or down.
    Backup,
    /// Send one console line — only while the server is running.
    Command { line: String },
}

/// A schedule as clients see it: the stored record plus the daemon's
/// computed hint for the next fire. The hint is informational (it assumes
/// the spec and the daemon's clock stay as they are); firing decisions are
/// always re-evaluated from the spec at tick time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleView {
    #[serde(flatten)]
    pub schedule: Schedule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_ms: Option<i64>,
}

/// `schedules.list {serverId}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulesListParams {
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulesListResult {
    pub server_id: String,
    pub schedules: Vec<ScheduleView>,
}

/// `schedules.create {serverId, name, spec, action, enabled}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulesCreateParams {
    pub server_id: String,
    pub name: String,
    pub spec: ScheduleSpec,
    pub action: ScheduleAction,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulesCreateResult {
    pub server_id: String,
    pub schedule: ScheduleView,
}

/// `schedules.update {serverId, scheduleId, name?, spec?, action?, enabled?}`
/// — absent fields keep their stored values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulesUpdateParams {
    pub server_id: String,
    pub schedule_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<ScheduleSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ScheduleAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulesUpdateResult {
    pub server_id: String,
    pub schedule: ScheduleView,
}

/// `schedules.delete {serverId, scheduleId}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulesDeleteParams {
    pub server_id: String,
    pub schedule_id: String,
}
