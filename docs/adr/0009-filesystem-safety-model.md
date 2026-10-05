# ADR-0009 — Filesystem safety model

**Status:** Accepted · **Date:** 2026-10-05

## Context

Core performs file operations on real server data at the request of UIs. A generic VFS abstraction is unwanted; a security model is not optional. The threats include a confused client, a malicious archive, symlinked paths, and ordinary cross-platform data hazards.

## Decision

One abstraction: the **rooted server filesystem**. Every operation resolves against a server root and is checked after canonicalization; anything outside the root is denied with a typed `FS_*` error.

Day-one requirements (not "later hardening"):

- **Path traversal protection** — canonicalize, then verify containment.
- **Symlink escape protection** — symlinks resolving outside the root are denied by default, with a visible indicator in the file manager and an explicit opt-in.
- **Atomic writes** — temp file + rename (+ fsync on Linux); retry on Windows sharing violations before failing.
- **Zip-slip protection** — archive entries are resolved and contained before extraction; total-size and entry-count limits apply.
- **Windows long paths** — `\\?\`-style handling internally; the app manifest declares long-path awareness.
- **Cross-platform restore handling** — Windows-reserved names (`CON`, `NUL`, `COM1`…), case-insensitive collisions, partial extraction (staging directory + commit/rollback), and disk-full are all handled, typed failures.
- **Watchers** — targeted at config and plugin files; **never a recursive watcher over `world/`**; debounce.
- **Listing** — depth-1 with lazy expansion; large operations run as jobs and never block an interactive path.
- **Backups of live servers** use `save-off` / `save-all flush` windows and handle Windows locked files explicitly.

## Consequences

- The file manager and every future bulk operation share one containment implementation, which is the only place containment logic can live.
- Security tests for this model are part of the required test suite (see [TESTING.md](../development/TESTING.md)).
