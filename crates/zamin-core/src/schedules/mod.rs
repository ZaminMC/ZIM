//! Server schedules (ADR-0014): the daemon runs the clock. This module
//! owns the three pieces that must be exactly right and testable without
//! a live daemon:
//!
//! - **Validation** — a schedule is refused at the protocol edge, before
//!   it is ever stored; garbage never reaches the clock.
//! - **Time math** — pure functions over (epoch ms, local offset, last
//!   fired): no chrono, no hidden clock, every caller injects the now.
//!   Firing decisions re-evaluate the spec every tick; nothing is
//!   precomputed and trusted.
//! - **The store** — `schedules.json` beside the server's other daemon
//!   metadata (registry precedent): versioned, atomically written, corrupt
//!   files are loud errors, never silent resets (ADR-0007).
//!
//! Timezone honesty: "daily at 04:30" means the daemon's local time. The
//! daemon computes the current offset once per tick and hands it in; the
//! math here is offset-parameterized so tests can pin any zone. A display
//! hint ("next run") assumes the current offset holds — firing decisions
//! never do.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use zamin_protocol::schedules::{Schedule, ScheduleAction, ScheduleSpec};

use crate::error::CoreError;
use crate::fsops::atomic_write;

pub const SCHEDULES_SCHEMA_VERSION: u32 = 1;

/// Wire cap so a listing can never grow a novel. Enough for honest names.
pub const MAX_NAME_CHARS: usize = 80;
/// One console line; a script belongs in a file, not a schedule.
pub const MAX_COMMAND_CHARS: usize = 256;
/// The protocol's floor for `every_secs`. The panel and CLI nudge
/// operators toward 300+; the wire stays honest — a 1-second interval is
/// dumb, not invalid, and it is what makes e2e tests possible.
pub const MIN_INTERVAL_SECS: u64 = 1;

/// Validate a name: trimmed, non-empty, capped.
pub fn validate_name(name: &str) -> Result<String, CoreError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CoreError::InvalidSchedule {
            reason: "the schedule needs a name".into(),
        });
    }
    if trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(CoreError::InvalidSchedule {
            reason: format!("the name is over {MAX_NAME_CHARS} characters"),
        });
    }
    Ok(trimmed.to_owned())
}

/// Validate a command line: trimmed, non-empty, capped.
pub fn validate_command(line: &str) -> Result<String, CoreError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(CoreError::InvalidSchedule {
            reason: "a command schedule needs a console line".into(),
        });
    }
    if trimmed.chars().count() > MAX_COMMAND_CHARS {
        return Err(CoreError::InvalidSchedule {
            reason: format!("the command is over {MAX_COMMAND_CHARS} characters"),
        });
    }
    Ok(trimmed.to_owned())
}

/// `"HH:MM"` in 24-hour local time → minutes since local midnight.
pub fn parse_hhmm(at: &str) -> Result<u32, CoreError> {
    let invalid = || CoreError::InvalidSchedule {
        reason: format!("\"{at}\" is not a 24-hour HH:MM time"),
    };
    let (h, m) = at.split_once(':').ok_or_else(invalid)?;
    if h.len() != 2
        || m.len() != 2
        || !(h.bytes().all(|b| b.is_ascii_digit()) && m.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid());
    }
    let hour: u32 = h.parse().map_err(|_| invalid())?;
    let minute: u32 = m.parse().map_err(|_| invalid())?;
    if hour > 23 || minute > 59 {
        return Err(invalid());
    }
    Ok(hour * 60 + minute)
}

/// Weekday wire form → 0 (Monday) .. 6 (Sunday).
pub fn parse_weekday(day: &str) -> Option<u32> {
    match day.trim().to_ascii_lowercase().as_str() {
        "mon" => Some(0),
        "tue" => Some(1),
        "wed" => Some(2),
        "thu" => Some(3),
        "fri" => Some(4),
        "sat" => Some(5),
        "sun" => Some(6),
        _ => None,
    }
}

/// The wire form of a weekday index ("mon".."sun").
pub fn weekday_wire(index: u32) -> &'static str {
    const NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    NAMES[(index as usize) % 7]
}

/// Validate the spec and expand it into the set of local minutes-of-week
/// (0 = Monday 00:00 .. 10079 = Sunday 23:59) it may fire in. Interval
/// specs are not minute-based and return an empty set — their due rule
/// is elapsed time, not the calendar.
pub fn allowed_minutes(spec: &ScheduleSpec) -> Result<BTreeSet<u32>, CoreError> {
    let mut out = BTreeSet::new();
    match spec {
        ScheduleSpec::Interval { every_secs } => {
            if *every_secs < MIN_INTERVAL_SECS {
                return Err(CoreError::InvalidSchedule {
                    reason: format!("every_secs must be at least {MIN_INTERVAL_SECS}"),
                });
            }
        }
        ScheduleSpec::Daily { at } => {
            let minute = parse_hhmm(at)?;
            for day in 0..7 {
                out.insert(day * 1440 + minute);
            }
        }
        ScheduleSpec::Weekly { weekdays, at } => {
            let minute = parse_hhmm(at)?;
            if weekdays.is_empty() {
                return Err(CoreError::InvalidSchedule {
                    reason: "a weekly schedule needs at least one weekday".into(),
                });
            }
            let mut days = Vec::new();
            for day in weekdays {
                let index = parse_weekday(day).ok_or_else(|| CoreError::InvalidSchedule {
                    reason: format!("\"{day}\" is not a weekday (mon..sun)"),
                })?;
                if !days.contains(&index) {
                    days.push(index);
                }
            }
            days.sort_unstable();
            for day in days {
                out.insert(day * 1440 + minute);
            }
        }
    }
    Ok(out)
}

/// Validate an action's payload (kinds are exhaustive by construction).
pub fn validate_action(action: &ScheduleAction) -> Result<ScheduleAction, CoreError> {
    match action {
        ScheduleAction::Restart | ScheduleAction::Backup => Ok(action.clone()),
        ScheduleAction::Command { line } => Ok(ScheduleAction::Command {
            line: validate_command(line)?,
        }),
    }
}

/// The local calendar minute containing `now_ms`, as (minute-of-week,
/// epoch ms of that minute's start). `offset_secs` is the local zone's
/// east-positive offset — the daemon reads the system's current offset;
/// tests pin one.
///
/// Derived from the epoch: 1970-01-01 was a Thursday, so
/// `weekday = (days + 3) % 7` with Monday = 0.
pub fn local_minute(now_ms: i64, offset_secs: i64) -> (u32, i64) {
    let local_secs = now_ms.div_euclid(1000) + offset_secs;
    let days = local_secs.div_euclid(86_400);
    let secs_of_day = local_secs.rem_euclid(86_400);
    let weekday = (days + 3).rem_euclid(7) as u32;
    let minute_of_day = (secs_of_day / 60) as u32;
    let minute_start_local = days * 86_400 + i64::from(minute_of_day) * 60;
    let minute_start_utc_ms = (minute_start_local - offset_secs) * 1000;
    (weekday * 1440 + minute_of_day, minute_start_utc_ms)
}

/// The clock one tick hands the math: a single now and the local
/// calendar minute that contains it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub now_ms: i64,
    /// Epoch ms of the local minute containing `now_ms`.
    pub minute_start_ms: i64,
    /// That minute's index in the week: 0 = Monday 00:00 .. 10079.
    pub minute_of_week: u32,
}

impl Tick {
    /// The tick for `now_ms` in the zone described by `offset_secs`
    /// (east-positive; the daemon passes the system's current offset).
    pub fn at(now_ms: i64, offset_secs: i64) -> Tick {
        let (minute_of_week, minute_start_ms) = local_minute(now_ms, offset_secs);
        Tick {
            now_ms,
            minute_start_ms,
            minute_of_week,
        }
    }
}

/// A schedule's own context: its spec, the expanded allowed minutes, the
/// stored record's timestamps, and the daemon's boot instant (interval
/// specs re-anchor there, so downtime never stacks firings).
#[derive(Debug, Clone, Copy)]
pub struct DueContext<'a> {
    pub spec: &'a ScheduleSpec,
    pub allowed: &'a BTreeSet<u32>,
    pub created_ms: i64,
    pub last_fired_ms: Option<i64>,
    pub anchor_ms: i64,
}

/// Is this schedule due, right now? Calendar kinds are due when the
/// current local minute is one of theirs and it was not already fired
/// (lastFired guards the tick's 15-second re-entry into the same minute).
/// Intervals are due on elapsed time.
pub fn is_due(ctx: &DueContext<'_>, tick: &Tick) -> bool {
    match ctx.spec {
        ScheduleSpec::Interval { every_secs } => {
            let base = ctx
                .last_fired_ms
                .unwrap_or(ctx.created_ms)
                .max(ctx.anchor_ms);
            tick.now_ms >= base + (*every_secs as i64) * 1000
        }
        _ => {
            ctx.allowed.contains(&tick.minute_of_week)
                && match ctx.last_fired_ms {
                    None => true,
                    Some(fired) => fired < tick.minute_start_ms,
                }
        }
    }
}

/// The display hint: when would this fire next, if nothing changed.
/// Assumes the current local offset holds (firing decisions never do —
/// they re-evaluate every tick). Calendar kinds scan forward minute by
/// minute over one week; intervals answer elapsed time.
pub fn next_run_hint(ctx: &DueContext<'_>, tick: &Tick) -> Option<i64> {
    match ctx.spec {
        ScheduleSpec::Interval { every_secs } => {
            let base = ctx
                .last_fired_ms
                .unwrap_or(ctx.created_ms)
                .max(ctx.anchor_ms);
            Some((base + (*every_secs as i64) * 1000).max(tick.now_ms))
        }
        _ => {
            for delta in 0..=10_080i64 {
                let minute = (i64::from(tick.minute_of_week) + delta).rem_euclid(10_080) as u32;
                if ctx.allowed.contains(&minute) {
                    if delta == 0
                        && ctx
                            .last_fired_ms
                            .is_some_and(|fired| fired >= tick.minute_start_ms)
                    {
                        continue; // this minute already had its fire
                    }
                    return Some(tick.minute_start_ms + delta * 60_000);
                }
            }
            None
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SchedulesFile {
    schema_version: u32,
    schedules: Vec<Schedule>,
}

/// The per-server schedule store: `<data>/servers/<id>/schedules.json`,
/// registry rules (load-absent-is-empty, loud corruption, atomic writes).
pub fn load_schedules(path: &Path) -> Result<Vec<Schedule>, CoreError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let file: SchedulesFile =
        serde_json::from_str(&text).map_err(|e| CoreError::SchedulesCorrupt {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    if file.schema_version != SCHEDULES_SCHEMA_VERSION {
        return Err(CoreError::SchemaVersion {
            path: path.to_path_buf(),
            found: file.schema_version,
            expected: SCHEDULES_SCHEMA_VERSION,
        });
    }
    Ok(file.schedules)
}

/// Atomically persist the store, stamping the schema version.
pub fn save_schedules(path: &Path, schedules: &[Schedule]) -> Result<(), CoreError> {
    let file = SchedulesFile {
        schema_version: SCHEDULES_SCHEMA_VERSION,
        schedules: schedules.to_vec(),
    };
    let mut bytes = serde_json::to_vec_pretty(&file).map_err(|e| CoreError::SchedulesCorrupt {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

/// Where a server's store lives, given the daemon's data dir and id.
pub fn schedules_path(data_dir: &Path, server_id: &str) -> PathBuf {
    data_dir
        .join("servers")
        .join(server_id)
        .join("schedules.json")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const OFFSET: i64 = 5 * 3600; // UTC+05:00, the host's zone shape

    /// 2026-10-06 was a Tuesday, epoch day 20_732. Builds the epoch ms of
    /// that day's `h:mm` local (UTC+05:00).
    fn tuesday_ms(h: u32, m: u32) -> i64 {
        (20_732 * 86_400 + (h * 3600 + m * 60) as i64 - OFFSET) * 1000
    }

    #[test]
    fn local_minute_places_a_known_instant() {
        // Tuesday 2026-10-06 04:30 local (UTC+05:00) → day 1 (Tue), 04:30.
        let now = tuesday_ms(4, 30);
        let (mow, start) = local_minute(now, OFFSET);
        assert_eq!(mow, 1440 + 4 * 60 + 30, "Tuesday 04:30 local");
        assert_eq!(start, now - now.rem_euclid(60_000), "minute start");
        // The start maps back to the same minute.
        let (mow2, start2) = local_minute(start + 1, OFFSET);
        assert_eq!((mow, start), (mow2, start2));
    }

    #[test]
    fn local_minute_survives_zero_and_negative_offsets() {
        // UTC: Tuesday 2026-10-06 04:30Z is still Tuesday locally.
        let utc_ms = (20_732 * 86_400 + 4 * 3600 + 30 * 60) * 1000;
        let (mow, _) = local_minute(utc_ms, 0);
        assert_eq!(mow, 1440 + 270);
        // UTC-08:00: the same instant is Monday 20:30 local.
        let (mow_west, _) = local_minute(utc_ms, -8 * 3600);
        assert_eq!(mow_west, 20 * 60 + 30, "Monday 20:30");
    }

    #[test]
    fn hhmm_rules() {
        assert_eq!(parse_hhmm("04:30").unwrap(), 270);
        assert_eq!(parse_hhmm("00:00").unwrap(), 0);
        assert_eq!(parse_hhmm("23:59").unwrap(), 1439);
        for bad in ["24:00", "4:30", "04:60", "ab:cd", "0430", "04:3", ""] {
            assert!(parse_hhmm(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn weekday_rules() {
        assert_eq!(parse_weekday("mon"), Some(0));
        assert_eq!(parse_weekday("SUN"), Some(6));
        assert_eq!(parse_weekday(" monday "), None);
        assert_eq!(parse_weekday("funday"), None);
        assert_eq!(weekday_wire(0), "mon");
        assert_eq!(weekday_wire(6), "sun");
    }

    #[test]
    fn daily_expands_to_seven_slots() {
        let spec = ScheduleSpec::Daily { at: "04:30".into() };
        let slots = allowed_minutes(&spec).unwrap();
        assert_eq!(slots.len(), 7);
        assert!(slots.contains(&(3 * 1440 + 270))); // Monday
        assert!(slots.contains(&(6 * 1440 + 270))); // Sunday
    }

    #[test]
    fn weekly_expands_to_listed_days_sorted_unique() {
        let spec = ScheduleSpec::Weekly {
            weekdays: vec!["sun".into(), "sat".into(), "sat".into()],
            at: "09:00".into(),
        };
        let slots = allowed_minutes(&spec).unwrap();
        assert_eq!(slots.len(), 2);
        assert!(slots.contains(&(5 * 1440 + 540)));
        assert!(slots.contains(&(6 * 1440 + 540)));
    }

    #[test]
    fn spec_validation_refuses_garbage() {
        assert!(allowed_minutes(&ScheduleSpec::Interval { every_secs: 0 }).is_err());
        assert!(allowed_minutes(&ScheduleSpec::Daily { at: "25:00".into() }).is_err());
        assert!(allowed_minutes(&ScheduleSpec::Weekly {
            weekdays: vec![],
            at: "04:00".into()
        })
        .is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_command("  \t ").is_err());
        let long = "x".repeat(MAX_COMMAND_CHARS + 1);
        assert!(validate_command(&long).is_err());
        // Trimmed round trip.
        assert_eq!(validate_name("  nightly \n").unwrap(), "nightly");
        assert_eq!(
            validate_action(&ScheduleAction::Command {
                line: " say hi ".into()
            })
            .unwrap(),
            ScheduleAction::Command {
                line: "say hi".into()
            }
        );
    }

    fn daily_at(h: u32, m: u32) -> ScheduleSpec {
        ScheduleSpec::Daily {
            at: format!("{h:02}:{m:02}"),
        }
    }

    #[test]
    fn calendar_due_guards_same_minute() {
        let spec = daily_at(4, 30);
        let allowed = allowed_minutes(&spec).unwrap();
        let now = tuesday_ms(4, 30);
        let tick = Tick::at(now, OFFSET);
        assert_eq!(tick.minute_of_week, 1440 + 270, "Tuesday 04:30");
        let due = |last_fired, now| {
            is_due(
                &DueContext {
                    spec: &spec,
                    allowed: &allowed,
                    created_ms: 0,
                    last_fired_ms: last_fired,
                    anchor_ms: 0,
                },
                &Tick::at(now, OFFSET),
            )
        };

        // Not yet fired this minute → due.
        assert!(due(None, now));
        // Fired 10 s into the minute → the tick re-enters and must skip.
        assert!(!due(Some(now + 10_000), now + 10_000));
        // Fired yesterday → due again.
        assert!(due(Some(now - 86_400_000), now));
        // Wrong minute → never due.
        assert!(!due(None, tuesday_ms(4, 31)));
    }

    #[test]
    fn interval_due_anchors_at_boot_and_remembers() {
        let spec = ScheduleSpec::Interval { every_secs: 60 };
        let empty = BTreeSet::new();
        let due = |last_fired, created, anchor, now| {
            is_due(
                &DueContext {
                    spec: &spec,
                    allowed: &empty,
                    created_ms: created,
                    last_fired_ms: last_fired,
                    anchor_ms: anchor,
                },
                &Tick {
                    now_ms: now,
                    minute_start_ms: 0,
                    minute_of_week: 0,
                },
            )
        };
        // Created long ago, daemon booted 30 s ago: anchored at boot,
        // due at boot+60.
        let now = 100_000;
        assert!(!due(None, 1_000, now - 30_000, now));
        assert!(due(None, 1_000, now - 30_000, now + 31_000));
        // Fired 10 s ago with no boot anchor: due at fire+60.
        assert!(!due(Some(now - 10_000), 0, 0, now));
        assert!(due(Some(now - 10_000), 0, 0, now + 50_000));
        // Downtime stacking: created at 0, last fired at 1000, boot long
        // after — the boot anchor wins over the stale lastFired.
        assert!(!due(Some(1_000), 0, 999_000, 1_000_000));
    }

    #[test]
    fn next_run_hint_scans_forward() {
        let spec = daily_at(4, 30);
        let allowed = allowed_minutes(&spec).unwrap();
        let hint = |spec, allowed, last_fired, now| {
            next_run_hint(
                &DueContext {
                    spec: &spec,
                    allowed: &allowed,
                    created_ms: 0,
                    last_fired_ms: last_fired,
                    anchor_ms: 0,
                },
                &Tick::at(now, OFFSET),
            )
        };
        // Tuesday 05:00 → next is Wednesday 04:30.
        let now = tuesday_ms(5, 0);
        assert_eq!(
            hint(spec.clone(), allowed.clone(), None, now),
            Some(now + (86_400 - 1_800) * 1000)
        );
        // The same minute, already fired → next day, not this minute.
        let now2 = tuesday_ms(4, 30);
        assert_eq!(
            hint(spec.clone(), allowed.clone(), Some(now2), now2),
            Some(now2 + 86_400_000)
        );
        // Interval hint: one period out, never in the past.
        let iv = ScheduleSpec::Interval { every_secs: 30 };
        let empty = BTreeSet::new();
        assert_eq!(
            hint(iv.clone(), empty.clone(), Some(90_000), 100_000),
            Some(120_000)
        );
        assert_eq!(
            hint(iv, empty, Some(1_000), 100_000),
            Some(100_000),
            "overdue hint clamps to now"
        );
    }

    #[test]
    fn store_round_trips_and_is_loud_about_corruption() {
        // Same helper discipline as the registry's tests: a scoped temp
        // dir, no dev-dependency.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "zamin-core-schedules-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("schedules.json");
        assert!(load_schedules(&path).unwrap().is_empty(), "absent is empty");

        let schedule = Schedule {
            id: "018f6d2e-aaaa-7bbc-b3aa-c1e2d3f4a5b6".into(),
            name: "nightly".into(),
            spec: ScheduleSpec::Daily { at: "04:30".into() },
            action: ScheduleAction::Restart,
            enabled: true,
            created_ms: 1_000,
            last_fired_ms: None,
        };
        save_schedules(&path, std::slice::from_ref(&schedule)).unwrap();
        let loaded = load_schedules(&path).unwrap();
        assert_eq!(loaded, vec![schedule]);

        std::fs::write(&path, "{not json").unwrap();
        assert!(load_schedules(&path).is_err(), "corrupt is loud");
        std::fs::write(&path, r#"{"schemaVersion":99,"schedules":[]}"#).unwrap();
        assert!(load_schedules(&path).is_err(), "wrong version is loud");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
