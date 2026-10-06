# Zamin Protocol v0

*Status: specification draft, ADR-0002 · The protocol is the client boundary. Clients know this document; they never know Core's internals.*

---

## 1. Transport

- A Zamin Protocol connection is a **byte stream** carrying length-prefixed UTF-8 JSON messages: `u32` little-endian byte count, then the message.
- Local transports: Windows named pipe (per-user name, ADR-0001) and Unix domain socket (`$XDG_RUNTIME_DIR/zamind/zamind.sock`). The framing is transport-agnostic; a future remote transport (TLS socket) uses the same framing.
- One connection multiplexes everything: requests, responses, and notification streams. No message batching at the protocol layer in v0.
- JSON objects are **tolerantly read**: unknown fields are ignored by both sides. Senders must not rely on field order.

## 2. Handshake (mandatory, first exchange)

```json
→ {"jsonrpc":"2.0","id":1,"method":"daemon.hello",
   "params":{"protocol":1,"auth":null,
             "client":{"name":"zamin-cli","version":"0.1.0"}}}

← {"jsonrpc":"2.0","id":1,"result":{
     "protocol":1,"protocolMin":1,"protocolMax":1,
     "daemon":{"name":"zamind","version":"0.1.0"},
     "capabilities":["server.lifecycle","streams","jobs"]}}
```

- `protocol` is an integer, starting at 1. Additive evolution does not change it; breaking changes bump it and define a `protocolMin`/`protocolMax` window.
- On mismatch the daemon replies with error `PROTOCOL_VERSION_UNSUPPORTED` and closes the connection. Clients render this as an upgrade prompt, never as a generic failure.
- `auth` is opaque in v0; local transports ignore it. A remote transport will define it. This is the authentication hook — nothing more is built now.

## 3. Requests, responses, notifications

- JSON-RPC 2.0. Request `id` is a u64 or a string; clients use monotonically increasing integers.
- **Every mutating request** (`server.*`, `job.cancel`, `file.*` writes) carries a `requestId` (UUIDv7, client-generated) inside `params`. The daemon deduplicates: an in-flight or recently completed (bounded LRU, ~10 min TTL) request ID returns the original outcome instead of re-executing. Request-ID dedupe is a client-retry safety net, not a transaction log.
- Notifications are daemon → client only in v0 (streams, §6).
- Cancellation of an operation = `job.cancel` for jobs, or `unsubscribe` for streams. Plain requests are not cancellable in v0; anything long-running must be a job.

## 4. Errors

```json
{"code":"PORT_IN_USE",
 "message":"Port 25565 is already in use by server 'production'.",
 "context":{"port":25565,"heldByServerId":"production"},
 "remediation":["choose_another_port","stop_managed_server"]}
```

- `code` is a stable `SCREAMING_SNAKE_CASE` string, registered as an enum in `zamin-protocol`. Prefixes: `DAEMON_`, `PROTOCOL_`, `SERVER_`, `PORT_`, `JAVA_`, `FS_`, `JOB_`, `CONFIG_`.
- `message` is a complete, specific, human-readable sentence (see the style guide's message rules).
- `remediation` lists action IDs clients may map to UI affordances; unknown IDs are ignored.
- JSON-RPC envelope failures answer `PROTOCOL_INVALID_REQUEST` (unreadable request id → the reply carries a null id, per JSON-RPC 2.0 §4.1) and `PROTOCOL_METHOD_NOT_FOUND` (no such method on this daemon's surface).
- Initial registry (non-exhaustive): `PROTOCOL_VERSION_UNSUPPORTED`, `PROTOCOL_INVALID_REQUEST`, `PROTOCOL_METHOD_NOT_FOUND`, `DAEMON_BUSY`, `SERVER_NOT_FOUND`, `SERVER_ID_EXISTS`, `SERVER_ID_INVALID`, `SERVER_ALREADY_RUNNING`, `SERVER_NOT_RUNNING`, `SERVER_START_TIMEOUT`, `PREFLIGHT_FAILED`, `NEEDS_EULA`, `JAVA_NOT_FOUND`, `JAVA_INCOMPATIBLE`, `JAVA_EXEC_FAILED`, `PORT_IN_USE`, `FS_OUTSIDE_ROOT`, `FS_NOT_WRITABLE`, `FS_NOT_FOUND`, `FS_PATH_ESCAPES_ROOT`, `ARCHIVE_UNSAFE_ENTRY`, `DISK_FULL`, `JOB_NOT_FOUND`, `JOB_NOT_CANCELLABLE`, `INTERNAL_ERROR`.

## 5. Method catalog (v0)

Implemented for the daemon's first release; the file set is specified now, implemented with the file manager.

| Namespace | Methods |
|---|---|
| `daemon` | `daemon.hello`, `daemon.status` |
| `server` | `server.list`, `server.get`, `server.register` (register an existing directory), `server.update`, `server.remove`, `server.start`, `server.stop`, `server.restart`, `server.kill`, `server.stdin` (one console line; output arrives on the logs stream) |
| `jobs` | `jobs.list`, `jobs.get`, `jobs.cancel` |
| `files` (Phase 4) | `files.list`, `files.read`, `files.write`, `files.mkdir`, `files.rename`, `files.delete`, `files.chunks` semantics below |
| `logs` | `logs.range` (file-backed historical read) |
| `streams` | `streams.subscribe`, `streams.unsubscribe` |

- `logs.range` is the file-backed historical read: the tail of the server's own `logs/latest.log`, through the rooted filesystem (ADR-0006's catch-up path).

```json
→ {"jsonrpc":"2.0","id":10,"method":"logs.range",
   "params":{"serverId":"production","maxLines":200}}

← {"jsonrpc":"2.0","id":10,"result":{
     "file":"logs/latest.log",
     "lines":[{"tsMs":0,"level":"info","thread":"Server thread",
               "line":"Done (3.214s)! For help, type \"help\""}],
     "olderAvailable":true}}
```

- `maxLines` defaults to 200 and is capped at 5000; the result is chronological and always the LAST `maxLines` lines ending at or before the requested offset.
- `startOffset` is the byte offset where the first returned line starts — a line boundary. Paging backward passes it as `beforeOffset`; a `startOffset` of 0 means the file holds nothing older.
- `olderAvailable` is exactly `startOffset > 0` — a scroll-up affordance, not an error.
- Reads walk the file backward in bounded windows (8 MiB), so a page costs roughly the page plus one window regardless of file size; a tail never re-reads from offset 0.
- A `beforeOffset` beyond the file's current length means rotation or truncation happened; the daemon answers a typed `LOG_CURSOR_INVALID` and the client restarts from the tail. A missing log file is a typed `FS_NOT_FOUND`, never a silent empty result.

- `server.start/stop/restart/kill` return immediately with the accepted state or a typed error; long outcomes arrive as `server.state_changed` events.
- `server.kill` is the force path (ADR-0005 ladder, step 4). `stop` is the graceful path. The verbs are fixed: **start, stop, restart, kill**. "Launch" and "terminate" are not protocol words.
- `server.register` exists in v0 because creating servers by download arrives with the software catalog (Phase 6); registration of existing directories is the honest Phase 1–2 path.

## 6. Streams and cursors

```json
→ {"jsonrpc":"2.0","id":9,"method":"streams.subscribe",
   "params":{"stream":"logs","serverId":"production","cursor":null}}
← {"jsonrpc":"2.0","id":9,"result":{"subscriptionId":"s1","cursor":{"file":"latest.log","offset":1048576},"batch":[]}}
← {"jsonrpc":"2.0","method":"streams.notification",
   "params":{"stream":"logs","serverId":"production","seq":48211,
             "batch":[{"tsMs":1730803278123,"level":"INFO","thread":"Server thread","line":"Done (3.214s)! For help, type \"help\""}]}}
```

- Streams: `events`, `logs`, `metrics` (semantics per ADR-0006).
- Registry changes ride the `events` stream like any lifecycle event: `server.register` publishes `state_changed` (`unknown → not-running`, `reason:"registered"`) and `server.remove` publishes (`not-running → unknown`, `reason:"removed"`). Clients with an open subscription learn about new and gone servers without polling.
- `seq` is a u64, monotonic per server per stream, assigned once at ingest.
- Cursor models: `events` — `seq`-based against a bounded replay ring, older → `{cursorInvalid:true}` and the client re-snapshots. `logs` — `{file, offset}`; the daemon validates file identity (rotation changes identity) and answers `cursorInvalid` rather than serving garbage.
- `metrics` delivers latest-wins samples; history comes from an explicit range query.
- All streams are bounded per subscriber; slow consumers receive `{stream, missed:N}` markers.

## 7. Jobs

```json
{"jobId":"018f3d2a-…","kind":"backup.create","serverId":"production",
 "state":"running","progress":{"current":412,"total":1024,"unit":"MiB","message":null},
 "error":null,"createdAtMs":1730803200000,"startedAtMs":1730803201000,"endedAtMs":null}
```

- Job states: `queued → running → succeeded | failed | cancelled`.
- Events: `job.started`, `job.progress`, `job.completed` with `outcome: succeeded | failed | cancelled` and, on failure, a typed error. Three event types, not five — outcome is data, not an event kind.
- `kind` values in v0: `server.create` (registration scaffolding), `backup.create`, `backup.restore`, `archive.extract`. Download kinds arrive with the software catalog.
- `jobs.cancel` is a request: the job observes it at its next cancellation point; the state transition to `cancelled` is authoritative.

## 8. Files

- Paths in every `files.*` call are **server-root-relative**, POSIX-style (`plugins/EssentialsX.jar`). `..` and absolute paths are rejected (`FS_PATH_ESCAPES_ROOT`) — the rooted filesystem (ADR-0009) is the only filesystem. Symlinks that resolve outside the root are listed (with `symlinkOutside: true`) but every operation on them is denied.
- Reads and writes are chunked; clients never send or receive one giant frame.

```json
→ {"jsonrpc":"2.0","id":20,"method":"files.list",
   "params":{"serverId":"production","path":"plugins","offset":0,"limit":500}}

← {"jsonrpc":"2.0","id":20,"result":{
     "path":"plugins","total":2,
     "entries":[{"name":"EssentialsX","kind":"directory","modifiedMs":1730803200000},
                {"name":"EssentialsX.jar","kind":"file","sizeBytes":2048000,"modifiedMs":1730803200000}]}}
```

- `files.list` pages directories-first, alphabetical (the daemon's canonical order); `limit` defaults to 500 and is capped at 2000; `total` is the whole directory's count so clients know when to stop.
- `files.read {serverId, path, offset, maxBytes}` → `{data(base64), eof, totalBytes}` — `maxBytes` is capped at 1 MiB; `eof` is true when the chunk reaches the end of the file; an `offset` past the end is a typed `PROTOCOL_INVALID_REQUEST`.
- `files.write {serverId, stagingId?, content}` → `{stagingId, bytesStaged}` — the first chunk omits `stagingId` and opens a staging file under the server root (`.zamin-staging/`, contained like everything else); later chunks append. `content` is base64 (standard alphabet), capped at 1 MiB decoded per chunk.
- `files.commit {serverId, stagingId, target}` → `{path, sizeBytes}` — one atomic rename; readers of `target` see old or new content, never a partial file. A consumed or forged staging handle is a typed `FS_NOT_FOUND`.
- `files.mkdir {serverId, path}` creates with parents. `files.rename {serverId, from, to}` renames within the root. `files.delete {serverId, path}` removes a file or an EMPTY directory — recursive deletion is a job-sized operation and does not belong in a synchronous method.

## 9. Conventions

- **Timestamps**: Unix epoch milliseconds; field names end in `Ms` (`createdAtMs`).
- **IDs**: `serverId` per ADR-0004 grammar; `jobId` and `requestId` are UUIDv7 (time-ordered, sortable).
- **Booleans** read as predicates: `eulaAccepted`, `portAvailable`.
- **Enums** are lower-kebab in JSON (`"state":"not-running"`), mapped to language enums in `zamin-protocol`.
- **Log line objects** carry the raw line text plus parsed fields (`tsMs`, `level`, `thread`); `tsMs` is the ingestion time — JVM log timestamps are local-time strings without offset and are not converted into false precision (raw string available in `logs.range` results).

## 10. Stability rules

1. Additive changes (new methods, new fields, new error codes, new job kinds, new capabilities) do not bump the protocol version.
2. Breaking changes bump the version and define `protocolMin`/`protocolMax`; clients and daemons negotiate and report actionable upgrade errors.
3. Every release ships protocol conformance fixtures (recorded exchanges) asserted in CI on both platforms.
4. The method catalog grows only when a second client needs the method. Panel-only conveniences do not enter the protocol "because they might be useful."
