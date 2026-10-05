# ADR-0004 — Server identity and registry

**Status:** Accepted · **Date:** 2026-10-05

## Context

Servers must be addressable by stable identity across Panel restarts, daemon restarts, and (later) machines. The filesystem location must never be the identity.

## Decision

- **`serverId`** is a slug: `^[a-z0-9][a-z0-9_-]{0,63}$`, unique per daemon, **immutable** after registration. `displayName` is mutable. Address/port/version live in server config, not in identity.
- **Registry** lives in app data: `registry.json` (JSON, versioned), one entry per server: `{serverId, displayName, rootPath, createdAtMs, ...}`. App data location per platform: Windows `%LOCALAPPDATA%\ZaminPanel`, Linux `$XDG_DATA_HOME/zaminpanel` (XDG conventions).
- **Marker file**: every managed server root contains `.zamin/server.json` with `{schemaVersion, serverId}`. The marker is written at registration and verified at scan.
- **Rediscovery** = (1) verify each registry entry (root exists, marker matches, no `serverId` collision), (2) optionally scan configured roots for marker files whose `serverId` is absent from the registry (adopt-or-report to the client; never silently merge).
- **All persistent files are versioned** (`schemaVersion` field) with migrators registered from the first release — registry, per-server state, and configuration alike. Unknown fields are preserved on write.
- The registry is owned by the daemon. Clients read it through the protocol only.

## Consequences

- Moving a server directory on disk requires re-linking through the UI/CLI (explicit, not silent).
- A `serverId` collision between registry and marker is an error surfaced to the client, never auto-resolved.
- Absolute paths never cross the protocol; only the daemon and registry know `rootPath`.

## Alternatives considered

- Identity = path: breaks on rename/move and leaks filesystem assumptions into clients.
- Marker-only identity (no registry): loses displayName, config, and per-server state.
