# ZIM Glossary

*One name per concept. When a new concept appears, add it here in the same PR. Review rejects synonyms on sight.*

## Core concepts

| Term | Meaning | Not called |
|---|---|---|
| **server** | A managed Minecraft server installation: registry entry + root directory + configuration | instance, node, runtime, backend |
| **serverId** | Immutable slug identifying a server (ADR-0004) | name, key, uuid |
| **server root** | The server's directory on disk | server path, server folder, home |
| **daemon** / **`zamind`** | The resident process that owns all server supervision and state | backend, service, server process, agent |
| **agent** | Reserved: the future remote component (ZaminAgent). Nothing else may use this word | — |
| **client** | A process holding a protocol session: Panel, CLI, (later) agent | frontend, consumer |
| **Zamin Protocol** | The JSON-RPC boundary between clients and the daemon | IPC API, internal API |
| **actor** | The per-server concurrency unit inside the daemon; sole owner of its server's runtime state | manager, controller, handler |
| **supervisor** | The core subsystem that owns actors and drives the lifecycle state machine | process manager, process controller |
| **ServerProcess** | The OS child process of a running server (the JVM) | runtime handle, managed runtime |
| **job** | A tracked long-running operation with progress and cancellation | task, operation, work item |
| **task** | Reserved: an async (tokio) task inside the daemon. Internal word only | — |
| **preflight** | Pre-spawn checks that produce typed failures instead of crashes | sanity check, validation gate |
| **adoption** | The daemon claiming an already-running server process after restart, after verifying identity | reattach, recovery, takeover |
| **identity (process)** | PID + start marker; both must match before adopt or kill (ADR-0005) | pid check |
| **attach / detach** | A client subscribing to / unsubscribing from a server's terminal stream | connect, hook in |
| **stream** | A subscribable notification channel: `events`, `logs`, `metrics` | feed, channel, topic |
| **cursor** | Opaque client position in a stream, valid until invalidated | bookmark, offset (except inside the logs cursor) |
| **snapshot** | A full current-state response used to bootstrap or resynchronize | dump, full state |
| **ring** | A bounded in-memory buffer (logs, metrics, event replay) | cache, buffer |
| **registry** | The daemon-owned index of managed servers (`registry.json`) | server list, database |
| **marker file** | `.zamin/server.json` inside a server root, binding the directory to its serverId | metadata file |
| **app data** | Platform-appropriate daemon-owned state directory | config folder |
| **software** | Server software family: Paper, Purpur, Folia, … | engine, platform, distro |
| **workspace** | Panel UI context for one open server (its tab + view state) | session, page |
| **tab** | The visual handle of a workspace | window, view |

## Verbs

| Use | Meaning | Not |
|---|---|---|
| **start** | Begin running a server | launch, boot, run, spin up |
| **stop** | Graceful shutdown (stdin `stop`, then the ladder) | shutdown, terminate, halt |
| **restart** | Stop then start, preserving the workspace | reboot, reload |
| **kill** | Force-terminate the OS process (ladder step 4) | murder, destroy, force-stop |
| **attach / detach** | Terminal stream subscription | connect to console |
| **adopt** | Claim a running process after identity verification | recover |
| **register** | Add an existing directory as a managed server | import, scan |

## Naming conventions

- Protocol JSON: `camelCase` fields, lower-kebab enum values. Rust: `snake_case`, types `PascalCase`. TS: `camelCase`, types `PascalCase`.
- Protocol fields referencing the identity are always `serverId` (JSON) / `server_id` (Rust). Never bare `id` in a message that contains other entities.
- Timestamps end in `Ms`. Booleans read as predicates (`eulaAccepted`).
- Error codes: `SCREAMING_SNAKE_CASE` with domain prefixes (see protocol spec §4).
- Crate and binary names: `zamin-protocol`, `zamin-ipc`, `zamin-core`, `zamind`, `zamin`, plus `apps/panel`. The word "ZIM" names the product and the GUI app, not the whole repository — the repo is the project home of all three.

## Process for changes

A new term enters through a PR that (1) adds it here, (2) uses it consistently in the same change, and (3) does not introduce a synonym for an existing term. Renaming an existing term is an ADR-level decision, not a drive-by.
