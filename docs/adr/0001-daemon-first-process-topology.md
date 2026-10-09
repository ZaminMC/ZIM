# ADR-0001 — Daemon-first process topology

**Status:** Accepted · **Date:** 2026-10-05

## Context

Requirements: servers keep running when Panel closes; rediscovery after Panel or daemon restart; multiple simultaneous clients (Panel + CLI); headless Linux; a future remote agent. A library embedded in Panel violates the first requirement; ownership transfer between processes on exit is race-prone and upgrade-fragile.

## Decision

ZaminCore runs as a resident daemon, **`zamind`** — one per operating-system user per machine. ZIM, ZaminCLI, and the future ZaminAgent are protocol clients. Nothing embeds `zamin-core` except `zamind`.

- **Endpoint naming is per-user.** Windows named pipes share a machine-global namespace, so the pipe and single-instance mutex names are derived from the user SID: `\\.\pipe\zamind-<sid-hash>`. Linux: `$XDG_RUNTIME_DIR/zamind/zamind.sock` (mode 0700), single-instance via a lockfile with a liveness check in the same directory.
- **Clients spawn the daemon transparently** if it is not running, then perform the protocol handshake (ADR-0002). Spawning must never be visible as an error path.
- **Daemon detachment.** On Linux, `zamind` detaches from the controlling terminal (or ignores SIGHUP) so a manually launched daemon survives SSH logout. On Windows, the daemon runs windowless.
- **Daemon lifetime.** Once started, the daemon stays resident (a config option may later allow exit-when-idle). The daemon never stops servers as a side effect of shutting down; servers keep running and the next daemon instance adopts them (ADR-0005).
- **Child processes are not tied to daemon lifetime:** Job Objects without kill-on-close on Windows, `setsid` process groups on Linux.
- Supervision code exists only in the daemon. There is no library-mode lifecycle path.

## Consequences

- Adoption after daemon restart is a first-class lifecycle concern (ADR-0005), not a recovery hack.
- Clients must handle a not-running daemon on every operation, including the reconnect path.
- Per-user endpoints mean two users on one machine get independent, isolated daemons.

## Alternatives considered

- **Core as a library inside Panel** — rejected: breaks detach, rediscovery, headless, multi-client.
- **Ownership transfer to a spawned supervisor on exit** — rejected: transfer windows, crash-during-transfer, version skew; strictly harder than a daemon with adoption.
