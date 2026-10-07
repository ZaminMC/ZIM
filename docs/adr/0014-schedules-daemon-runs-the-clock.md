# ADR-0014 — Schedules: the daemon runs the clock

**Status:** Accepted · **Date:** 2026-10-07

## Context

Every phase of the implementation order (§23) has landed, and the last two smokes (the live Modrinth update check, the retire step) found defects only a running product reveals. The remaining known items are all deliberate waits: Forge is demand-gated, the SQLite log index is deferred, the Model workspace is reserved. That makes this the right moment to take a named gap that operators of every serious Minecraft panel feel daily: **nothing in the product can run a scheduled task.** The nightly restart, the hourly world backup, the "restarting in 10 minutes" console warning — today these require an operator (or a host-level crontab speaking the CLI) to be awake, which on a headless box managed remotely is precisely the thing the panel was built to make unnecessary.

The daemon already owns everything a scheduler needs: per-server actors that take lifecycle commands and stdin, a job runner with progress and cancellation, an events hub, an audit log, and a filesystem-safety model. The architecture review's overengineering list (§17) warns against adding machinery before demand, and its under-abstraction list (§18) warns the opposite. The resolution is the same as it was for the plugin catalog: implement the demand that is real (restarts, backups, console lines on a clock), refuse the speculation (crons with arbitrary expressions, script execution, fleet-wide templates), and keep the surface additive.

## Decision

### The daemon runs the clock; clients author rules

A schedule is a named record per server: a **when** (interval, daily, weekly), a **then** (restart, backup, command), and an `enabled` bit. The records live in `<data>/servers/<id>/schedules.json` — registry rules apply: absent is empty, corruption is loud (`INTERNAL_ERROR`, never a silent reset), writes are atomic, `schemaVersion` from day one. The daemon spawns one tick loop (15 s); every tick it re-reads every store and re-evaluates every spec against the current local minute. There are **no cached timers**: created, updated, and deleted schedules land on the next tick; a daemon restart replays nothing; there is no scheduler state to flush or corrupt.

`lastFiredMs` is the schedule's only memory — the last dispatch that actually happened, persisted atomically. It is genuine scheduling state (the clock's own bookmark), not shadow state in ADR-0012's sense: nothing derived is pretended stored, and nothing stored pretends to be derived.

### Time math is pure, injected, and honest about zones

"Daily at 04:30" means **the daemon's local time** — the daemon is the only component that knows which machine it runs on, and the panel must not guess (a remote profile may sit in another zone). The zone arithmetic lives in `zamin_core::schedules` as pure functions over `(epoch ms, local offset, last fired)` with no hidden clock: the daemon computes the current offset once per tick (re-read every tick, so a DST change or a moved machine is picked up without a restart) and injects it; unit tests pin arbitrary zones and instants.

Three firing rules, each deliberate:

- **Interval** fires on elapsed time and re-anchors at daemon boot — downtime never stacks up firings. A missed nightly restart is skipped, not replayed at 10:30.
- **Calendar kinds** (daily, weekly) are due when the current local minute is one of theirs and `lastFired` predates that minute — the guard that keeps a 15 s tick from firing the same minute four times.
- **A schedule never switches a machine on.** A `restart` fires only while the server is Running (the actor's restart-from-stopped would start it, which is the operator's verb, not the clock's); a `command` fires only while Running (stdin has nowhere to go otherwise); a `backup` fires either way (quiet files are as safe to archive as saved ones). A skipped fire is not a fire: `lastFired` does not advance, the calendar's minute window simply closes.

### Dispatch rides the ordinary paths

A fire is an engine call — the same `restart` verb, the same backup job, the same stdin — so events, job records, and the audit look exactly like an operator's action, and the console line lands in the same log pipeline. The clock adds no new event kind and no new audit writer; `lastFiredMs` on the record and the action's own effects are the evidence. A dispatch failure the action itself owns (a backup that hits a full disk, a restart that races a stop) is the action's story to tell, not the clock's; the clock only remembers that it fired. In-flight actions are guarded by schedule id so a slow restart never overlaps its own next interval.

### The wire is additive and self-describing

`schedules.list/create/update/delete` join the catalog (§7e). Specs are internally tagged JSON (`{"kind": "daily", "at": "04:30"}`), so a future when-shape is additive under the v0 stability rules; the panel and CLI already render unknown-but-valid shapes from the list result. Validation happens at the daemon edge — names, `HH:MM`, weekdays, the interval floor (1 s on the wire, because an e2e test at 2 s is worth more than a rule the daemon pretends to enforce; the panel nudges 300+) — so garbage never reaches the store, and both clients refuse identically. Unknown schedule ids are typed `SCHEDULE_NOT_FOUND` on update and delete: a typo must not look like success. Views carry `nextRunMs` as an explicit display hint (it assumes the current zone holds); paused schedules carry no hint, because a fire that cannot happen must not be promised.

## Consequences

- The panel grows a Schedules tab (rows with the when in words, the action, the clock's memory; pause/resume/remove; an authoring form with local validation), and the CLI grows `zamin schedules list/add/remove/pause/resume` — the second client proves the surface is protocol-shaped, not UI-shaped.
- Schedules fire only while the daemon runs. This is stated, not hidden: the daemon-first topology (ADR-0001) is what makes the schedule trustworthy, and "missed firings are skipped" is the rule that keeps a week of downtime from becoming a firing storm at boot.
- The scheduler reads every store every tick. At the product's scale (tens of servers, handfuls of schedules) this is a few small file reads per 15 s — deliberately cheaper than any cache that could go stale.
- Rejected for now: cron expressions (a parser and a UX surface for power that no current demand names), multi-line commands (a script belongs in a file), fleet-wide schedule templates (team features, §22), and catch-up semantics (the boot re-anchor is simpler and safer). Each becomes a small ADR if real demand arrives.
