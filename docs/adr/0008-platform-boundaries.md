# ADR-0008 — Platform boundaries

**Status:** Accepted · **Date:** 2026-10-05

## Context

Windows and Linux are first-class from day one. The failure mode to prevent is platform assumptions leaking into Core, or Linux becoming "the Windows version plus fixes" six months in.

## Decision

### The seam

OS-specific code exists only in `crates/zamin-core/src/platform/` and `crates/zamin-ipc` (transport). The complete inventory:

| Concern | Windows | Linux |
|---|---|---|
| Spawn | argv array, `CREATE_NEW_PROCESS_GROUP`, hidden window | argv array, `setsid`, own process group |
| Graceful OS stop | CTRL_BREAK to group (verified early; see ADR-0005) | SIGTERM to group |
| Forced kill | TerminateProcess | SIGKILL to group |
| Child tracking | Job Object, no kill-on-close | process group |
| Process identity | PID + creation time | PID + starttime + boot id |
| IPC endpoint | per-user named pipe (SID-derived name) | UDS in `XDG_RUNTIME_DIR` |
| Single instance | SID-derived named mutex | lockfile + liveness check |
| App directories | Known Folders | XDG base dirs |
| Java discovery | registry + Program Files + env | `/usr/lib/jvm`, `/opt`, SDKMAN, asdf, env |
| Long paths | `\\?\` handles + longPathAware manifest | n/a |
| Permissions surfaced | ACL errors | mode-bit errors ("not writable by current user") |
| Autostart / service (later) | Task Scheduler / Run key | systemd user unit |
| Notifications (later) | toast | freedesktop Notifications |

### Enforcement

- `#[cfg(windows)]` / `#[cfg(unix)]` are permitted **only** inside `platform/` and `zamin-ipc`. A CI guard script fails the build on violations.
- CI runs build + clippy + tests on Windows and Linux on every push, from the first commit.
- No distro-specific branching anywhere (`if ubuntu` is a bug). Code cares about kernel, desktop, filesystem, and runtime differences, which all live in the table above.
- Normal operation never requires root or Administrator.
