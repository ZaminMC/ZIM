# ADR-0007 — Configuration model

**Status:** Accepted · **Date:** 2026-10-05

## Context

Configuration has three owners that must not be confused: ZaminPanel's own settings, per-server settings the daemon owns, and files Minecraft owns (`server.properties`, `eula.txt`). Retrofitted migration and silent file fights are the classic failure modes here.

## Decision

- **Formats.** TOML for human-edited configuration (comments matter); JSON for machine state (registry, job records). Every file carries `schemaVersion`.
- **Layering.** Global defaults in app data, per-server configuration layered over them. Every field carries provenance: `global` or `custom` — this is what lets the UI show "Using global default" vs "Custom value" without guessing.
- **Migrators from the first release.** A version bump without a migrator is a CI failure (test: open every fixture file from the previous version).
- **`server.properties` is owned by Minecraft.** Core never caches it across process lifetime, re-reads it immediately before any structured write, writes through the atomic path (temp + rename), and watches the specific config files (never `world/`) to warn on external modification. Desired values in server config are reconciled with the actual file **explicitly and with consent**, never silently.
- **EULA** is a preflight check, not a crash: a missing/unaccepted `eula.txt` yields typed `NEEDS_EULA` before spawn.

## Consequences

- No config migration debt accumulates invisibly; upgrading an old install is a test-covered path.
- The daemon and the server process never race on the same file with stale data.
