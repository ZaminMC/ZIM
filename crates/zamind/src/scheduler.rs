//! The scheduler (ADR-0014): the daemon runs the clock. One loop wakes
//! every tick, reads every registered server's schedule store, and fires
//! what is due — through the engine's ordinary paths, so a scheduled
//! restart is indistinguishable from an operator's (same events, same
//! audit shape, same job records).
//!
//! Policy, in one place:
//! - a `restart` fires only while the server is Running — a schedule
//!   never switches a machine on (the actor's restart would start it);
//! - a `command` fires only while Running (stdin has nowhere to go
//!   otherwise);
//! - a `backup` fires whether the server is up or down (quiet files are
//!   as safe to archive as saved ones).
//!
//! The clock never caches timers. Every tick re-reads the stores and
//! re-evaluates each spec against the current minute, so created,
//! updated, and deleted schedules land on the next tick, a daemon
//! restart replays nothing, and a hand-edited store is refused (loudly
//! logged, never fired) rather than trusted.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zamin_core::schedules::{allowed_minutes, is_due, DueContext};
use zamin_core::server::ServerId;
use zamin_protocol::schedules::{Schedule, ScheduleAction};
use zamin_protocol::server::ServerState;

use crate::engine::{now_ms_unix, Engine, LifecycleKind};

/// How often the clock looks at the world. Minute-granular specs need
/// this well under a fire window of one minute; 15 s gives every due
/// minute four chances while costing one small file read per server.
pub(crate) const SCHEDULER_TICK: Duration = Duration::from_secs(15);

/// Spawn the clock. One per daemon; the loop lives for the process.
pub(crate) fn spawn(engine: Engine) {
    tokio::spawn(async move {
        let firing: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        loop {
            tokio::time::sleep(SCHEDULER_TICK).await;
            tick(&engine, &firing).await;
        }
    });
}

/// One look at the world: every server, every schedule, fire what is due.
/// In-flight schedules are tracked by id so a slow restart never overlaps
/// its own next interval — the dispatch task removes its guard when done.
async fn tick(engine: &Engine, firing: &Arc<Mutex<HashSet<String>>>) {
    let now_ms = now_ms_unix();
    let offset_secs = crate::engine::local_offset_secs();
    let clock = zamin_core::schedules::Tick::at(now_ms, offset_secs);

    for server in engine.list_servers().await {
        let Ok(id) = ServerId::parse(&server.server_id) else {
            continue;
        };
        let schedules = match engine.schedules_load(&id) {
            Ok(schedules) => schedules,
            Err(e) => {
                // A corrupt store must not mute the rest of the fleet.
                tracing::error!("schedules for {id} are unreadable, skipping this tick: {e}");
                continue;
            }
        };
        for schedule in schedules {
            if !schedule.enabled {
                continue;
            }
            let Ok(allowed) = allowed_minutes(&schedule.spec) else {
                tracing::error!(
                    "schedule {}/{} carries an invalid spec; refusing to fire it \
                     (fix or remove it in the panel)",
                    id,
                    schedule.id
                );
                continue;
            };
            let due = is_due(
                &DueContext {
                    spec: &schedule.spec,
                    allowed: &allowed,
                    created_ms: schedule.created_ms,
                    last_fired_ms: schedule.last_fired_ms,
                    anchor_ms: engine.boot_ms(),
                },
                &clock,
            );
            if !due || !insert_firing(firing, &schedule.id) {
                continue;
            }
            let engine = engine.clone();
            let firing = firing.clone();
            let server_id = id.clone();
            tokio::spawn(async move {
                fire(&engine, &server_id, schedule, now_ms, &firing).await;
            });
        }
    }
}

/// Dispatch one action and remember the fire. The policy check comes
/// first — a skipped fire is not a fire (lastFired must not advance for
/// a server that is not running); the record lands once the action is
/// dispatched; the action's own outcome is the action's story (lifecycle
/// events, job records, the console), not the clock's.
async fn fire(
    engine: &Engine,
    server_id: &ServerId,
    schedule: Schedule,
    now_ms: i64,
    firing: &Arc<Mutex<HashSet<String>>>,
) {
    let outcome: Result<bool, String> = match &schedule.action {
        ScheduleAction::Restart => {
            if engine.describe_state(server_id).await != ServerState::Running {
                Ok(false) // a schedule never switches a machine on
            } else {
                engine
                    .lifecycle(server_id, LifecycleKind::Restart)
                    .await
                    .map(|_| true)
                    .map_err(|e| format!("restart failed: {e}"))
            }
        }
        ScheduleAction::Command { line } => {
            if engine.describe_state(server_id).await != ServerState::Running {
                Ok(false)
            } else {
                engine
                    .write_stdin(server_id, line.clone())
                    .await
                    .map(|_| true)
                    .map_err(|e| format!("command failed: {e}"))
            }
        }
        ScheduleAction::Backup => engine
            .backup_create(server_id, Some(schedule.name.clone()))
            .await
            .map(|_| true)
            .map_err(|e| format!("backup failed to start: {e}")),
    };

    match outcome {
        Ok(true) => {
            engine.schedule_mark_fired(server_id, &schedule.id, now_ms);
        }
        Ok(false) => {}
        Err(message) => tracing::warn!(
            "schedule {}/{} could not be dispatched: {message}",
            server_id,
            schedule.id
        ),
    }
    return_firing(firing, &schedule.id);
}

fn insert_firing(firing: &Mutex<HashSet<String>>, id: &str) -> bool {
    firing
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id.to_owned())
}

fn return_firing(firing: &Mutex<HashSet<String>>, id: &str) {
    firing
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(id);
}
