# ADR-0005 — Server lifecycle state machine and process safety

**Status:** Accepted · **Date:** 2026-10-05

## Context

The supervisor is the correctness core of the product. The state model must distinguish "known failure with a fixable cause" from "crash," must survive daemon restarts, and must never let a PID reuse kill an unrelated process.

## Decision

### States

`NOT_RUNNING → STARTING → RUNNING → STOPPING → STOPPED`, plus `FAILED_PREFLIGHT`, `CRASHED`, `ADOPTING`, `UNKNOWN`. Crash carries a classification: `{phase: startup | runtime | shutdown, exitCode, evidence}`. Attachment (clients attached to the terminal stream) is a separate dimension, not a state.

### Preflight

Before spawn, each check runs and reports independently: JAR exists and is readable; selected Java's major version satisfies the requirement; server root writable by the current user; EULA accepted (typed `NEEDS_EULA` — the UI renders an accept dialog); port available (ADR for ports lives with the server config); disk headroom sanity. Any failure → `FAILED_PREFLIGHT` with the typed error. No spawn on partial failure.

### Startup validation

`STARTING` ends at `RUNNING` on the software's startup signature (Paper family: the `Done (…)!` line) **or** the port listening, within a configurable timeout. Exit before validation → `CRASHED{phase: startup}`. Timeout → remain `STARTING` with progress surfaced; never guess.

### Shutdown ladder

1. stdin `stop`; 2. wait `stop_timeout` (default 60 s); 3. Linux: SIGTERM to the process group. Windows: CTRL_BREAK to the process group (spawn with `CREATE_NEW_PROCESS_GROUP`; the daemon shares the child's console). This step is verified in the first supervisor week; if unreliable for a headless daemon, the ladder degrades to step 4 and says so; 4. Linux: SIGKILL to the group. Windows: TerminateProcess.

### Process identity (hard safety invariant)

A process is "ours" only if **PID + start marker** both match the registry (Windows: creation time; Linux: `/proc/<pid>` starttime + boot id). On daemon restart with servers running: marker match → `ADOPTING → RUNNING`; PID alive but marker mismatch → **never adopt, never kill**, surface `UNKNOWN`. No code path may kill a process whose identity was not verified.

### Concurrency

One actor per server owns state, process, stdin, log pipeline, and metrics. The daemon coordinates; the actor is the only writer of its server's runtime state. Double-start, concurrent stdin, and state races are made structurally difficult, not policed by locks.

## Consequences

- `FAILED_PREFLIGHT` vs `CRASHED` is what makes actionable error UI possible.
- Adoption is exercised by tests from the first supervisor release (fake-mc-server lifecycle matrix).
