# Zamin Protocol v0

*Status: specification draft, ADR-0002 · The protocol is the client boundary. Clients know this document; they never know Core's internals.*

---

## 1. Transport

- A Zamin Protocol connection is a **byte stream** carrying length-prefixed UTF-8 JSON messages: `u32` little-endian byte count, then the message.
- Local transports: Windows named pipe (per-user name, ADR-0001) and Unix domain socket (`$XDG_RUNTIME_DIR/zamind/zamind.sock`). The framing is transport-agnostic; the remote transport (ADR-0011) uses the same framing over TLS TCP.
- **Remote transport** (ADR-0011): ZaminAgent listens on TCP with TLS 1.3 and relays to the local daemon. One remote connection maps to one daemon session. The agent is a frame-level bridge — it validates the handshake's auth and forwards everything else unchanged.
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
- `auth` is the authentication hook. **Local transports ignore it** — there the OS user + pipe/UDS ACL is the credential (documented, not ignored). **The remote transport requires it** (ADR-0011): the agent checks `auth` against its token before any daemon connection exists.
  - Missing/empty `auth` over a remote transport → error `AUTH_REQUIRED`, connection closed.
  - Wrong `auth` → error `AUTH_REJECTED`, connection closed.
  - A matching hello is forwarded unchanged; the daemon still ignores `auth` on its local leg.
  - If the agent cannot reach its local daemon → error `DAEMON_UNREACHABLE` (carrying the transport error), connection closed.
  - Both auth errors answer with the client's own request id; a first frame that is not a readable `daemon.hello` request answers with a Null id and `PROTOCOL_INVALID_REQUEST`, mirroring the daemon's malformed-frame rule (§3).

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
- Initial registry (non-exhaustive): `PROTOCOL_VERSION_UNSUPPORTED`, `PROTOCOL_INVALID_REQUEST`, `PROTOCOL_METHOD_NOT_FOUND`, `DAEMON_BUSY`, `DAEMON_UNREACHABLE`, `AUTH_REQUIRED`, `AUTH_REJECTED`, `SERVER_NOT_FOUND`, `SERVER_ID_EXISTS`, `SERVER_ID_INVALID`, `SERVER_ALREADY_RUNNING`, `SERVER_NOT_RUNNING`, `SERVER_START_TIMEOUT`, `PREFLIGHT_FAILED`, `NEEDS_EULA`, `JAVA_NOT_FOUND`, `JAVA_INCOMPATIBLE`, `JAVA_EXEC_FAILED`, `PORT_IN_USE`, `FS_OUTSIDE_ROOT`, `FS_NOT_WRITABLE`, `FS_NOT_FOUND`, `FS_PATH_ESCAPES_ROOT`, `ARCHIVE_UNSAFE_ENTRY`, `DISK_FULL`, `JOB_NOT_FOUND`, `JOB_NOT_CANCELLABLE`, `INTERNAL_ERROR`. `DAEMON_UNREACHABLE` / `AUTH_REQUIRED` / `AUTH_REJECTED` are raised by the remote transport (ADR-0011).

## 5. Method catalog (v0)

Implemented for the daemon's first release; the file set is specified now, implemented with the file manager.

| Namespace | Methods |
|---|---|
| `daemon` | `daemon.hello`, `daemon.status` |
| `server` | `server.list`, `server.get`, `server.register` (register an existing directory), `server.create` (Phase 6: download, stamp, and register a fresh server — §7b), `server.update`, `server.remove`, `server.start`, `server.stop`, `server.restart`, `server.kill`, `server.stdin` (one console line; output arrives on the logs stream), `server.discover` (§64, ADR-0027) |
| `discovery` (ADR-0027) | `discovery.roots.get`, `discovery.roots.set` |
| `extensions` (§56/§57, ADR-0031) | `extensions.list` — the declaration half of the permission model: valid manifests with their claimed permissions, folders that could not be read named in-band, `contributionsActive: false` until the execution model lands |
| `catalog` (Phase 6) | `catalog.list`, `catalog.versions`, `catalog.builds` (§7b) |
| `java` (Phase 6) | `java.list`, `java.install` (§7c) |
| `plugins` (§7d) | `plugins.search`, `plugins.versions`, `plugins.installed`, `plugins.install` (a job), `plugins.delete`, `plugins.updates` |
| `schedules` (§7e) | `schedules.list`, `schedules.create`, `schedules.update`, `schedules.delete` |
| `publish` (§7f) | `publish.config.get`, `publish.config.set`, `publish.providers.list`, `publish.preview`, `publish.execute` (a job), `publish.state`, `publish.review.set` |
| `config` (§7g) | `config.get`, `config.set` |
| `network` (§7g) | `network.status` |
| `jobs` | `jobs.list`, `jobs.get`, `jobs.cancel` |
| `backups` (Phase 5) | `backup.create`, `backup.restore`, `backups.list` |
| `files` (Phase 4) | `files.list`, `files.read`, `files.write`, `files.mkdir`, `files.rename`, `files.delete`, `files.chunks` semantics below |
| `logs` | `logs.range` (file-backed historical read) |
| `players` | `players.list` (Server List Ping: online/max, the server's name sample, latency, version, MOTD) |
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

- `server.discover {query?}` (ADR-0027) answers the founder's §64: the merged, typed list of what this machine holds — registry entries first (`kind:"registered"`, live `state`), then scanned `directory` rows (with `port` from `server.properties`) and `jar` rows (with `platform` as filename evidence), each with its absolute `path`. The merge never names one server twice: marker ids, registered roots, and anything inside a registered root are dropped from the scan side. Honesty fields ride along: `roots` (what was actually scanned), `skippedRoots` (named, not hidden), `scanned`, `truncated`.

- `players.list {serverId}` asks the server itself with a Server List Ping (vanilla flow, no plugins): `{source: "ping", online, max, sample: [{name, id?}], latencyMs, version?, motd?}`. The sample is the server's own preview (vanilla caps it at 12 names), not the full roster. A server that does not answer (off, starting) is an empty-room result with `online: null` — a normal state, never an error; a server with no port configured is a typed `PROTOCOL_INVALID_REQUEST`.

- `server.start/stop/restart/kill` return immediately with the accepted state or a typed error; long outcomes arrive as `server.state_changed` events.
- `server.kill` is the force path (ADR-0005 ladder, step 4). `stop` is the graceful path. The verbs are fixed: **start, stop, restart, kill**. "Launch" and "terminate" are not protocol words.
- `server.register` remains the bootstrap for directories that already exist. Creating servers by download is `server.create` (§7b, Phase 6); registration of existing directories is the honest Phase 1–2 path.

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
- `metrics` delivers latest-wins samples; history comes from the explicit `metrics.range` query:

```json
→ {"jsonrpc":"2.0","id":12,"method":"metrics.range",
   "params":{"serverId":"production","maxSamples":120}}
← {"jsonrpc":"2.0","id":12,"result":{"samples":[
     {"tsMs":1730803271000,"cpuPercent":12.4,"rssBytes":812000000,"players":3,"uptimeMs":45012},
     {"tsMs":1730803272000,"cpuPercent":11.9,"rssBytes":812300000,"players":3,"uptimeMs":46012}]}}
```

  - The daemon samples each live server process at 1 Hz: CPU% (from OS counter deltas — the first sample of a process carries no percent rather than a fake one), RSS, live player count, uptime. `tps` is only ever set when actually measured; the daemon never guesses.
  - `latest-wins`: a slow metrics subscriber keeps only the newest undelivered sample (no `missed` markers on this stream — a stale sample has no value to catch up to).
  - `metrics.range` returns the per-server ring, chronological (oldest first), bounded by design (600 samples ≈ 10 minutes at 1 Hz); the ring is the entire stored history — no paging. Unknown server → typed `SERVER_NOT_FOUND`.
- All streams are bounded per subscriber; slow consumers receive `{stream, missed:N}` markers (except `metrics`, which coalesces as above).

## 7. Jobs

```json
{"jobId":"018f3d2a-…","kind":"backup.create","serverId":"production",
 "state":"running","progress":{"current":412,"total":1024,"unit":"MiB","message":null},
 "error":null,"createdAtMs":1730803200000,"startedAtMs":1730803201000,"endedAtMs":null}
```

- Job states: `queued → running → succeeded | failed | cancelled`.
- Events: `job.started`, `job.progress`, `job.completed` with `outcome: succeeded | failed | cancelled` and, on failure, a typed error. Three event types, not five — outcome is data, not an event kind.
- `kind` values in v0: `server.create` (download-and-register, §7b), `backup.create`, `backup.restore`, `archive.extract`, `java.install` (§7c).
- `jobs.cancel` is a request: the job observes it at its next cancellation point; the state transition to `cancelled` is authoritative.
- Finished job history is bounded on the daemon (the oldest finished records drop first); a pruned id answers `JOB_NOT_FOUND`, which is a re-snapshot signal, not a protocol error.

### 7a. Backups (Phase 5)

- `backup.create {requestId, serverId, label?}` returns the running `Job` immediately. For a running server the archive walk is wrapped in the ADR-0009 save window (`save-off` → `save-all` → settle → walk → `save-on`; `save-on` runs even when the job fails or is cancelled). A cold backup skips the window. Retention (`backupKeep`, layered setting, built-in default 10) prunes after each success.
- `backup.restore {requestId, serverId, backupId}` returns the running `Job`. Refused with `SERVER_ALREADY_RUNNING` unless the server is `not-running` — files a running server holds open cannot be replaced. The extract goes through the full ADR-0009 trap list (zip-slip, Windows-reserved names, case-fold collisions, size/entry limits, link entries); commit failure rolls the previous files back (`RestoreRolledBack` surfaces as a typed error, never a half-restored root).
- `backups.list {serverId}` reads the per-server manifests, newest first: `{backups: [{backupId, createdAtMs, sizeBytes, totalBytes, fileCount, label?, taken: "live" | "cold"}]}`. An unknown `backupId` is a typed `FS_NOT_FOUND`.
- Archives live under the daemon's data dir (`backups/<serverId>/<backupId>.tar.gz` + `.json` manifests); clients never see paths — only ids.

### 7b. Software catalog & creation (Phase 6; second family per ADR-0013)

The catalog is **data**, not an abstraction (ARCH-REVIEW §17.4/§17.8): the daemon knows how to create what its catalog table lists, each row carries the upstream family it speaks, and the families are the PaperMC Fill API v3 (`https://fill.papermc.io/v3`; the legacy v2 API is retired upstream) and the FabricMC meta API v2 (ADR-0013). Every base URL is a daemon flag (`--catalog-url`, `--fabric-url`) — tests point them at mocks, air-gapped installs at mirrors.

- `catalog.list` → `{entries: [{id, name, description, source}]}`. Rows: `paper`, `purpur`, `folia` (`source: "fill"`) and `fabric` (`source: "fabric-meta"`). The `source` field is additive (v0) and tells clients which creation dialect applies — builds or loaders — without hard-coding ids.
- `catalog.versions {project}` → `{versions: [{id, javaMajor?}]}`, newest first. For the Fill family `javaMajor` is the software's own requirement when the catalog states one; for the Fabric family the daemon's local table decides (upstream publishes no Java requirements).
- `catalog.builds {project, version}` → `{javaMajor?, builds: [{id, channel, time?, download: {name, sha256, size?, url}}], loaders?}`, builds newest first, only builds publishing a `server:default` download. A Fill-family entry answers `builds` and no `loaders`. A Fabric-family entry answers an **empty** `builds` array with `loaders` — the stable loader versions, newest first — because Fabric has no numeric builds and none are invented; an unstable loader can still be pinned explicitly at creation. Clients never fetch `download.url` — the daemon downloads it itself.
- `server.create {requestId, serverId, displayName?, project, version, build?, loader?, templateId?, port?, javaPath?}` returns the running `Job` (`kind: "server.create"`). The daemon resolves the build (Fill) or the loader pin (Fabric; omit for the newest stable) **before** spawning the job, so unknown project/version/build/loader/template are typed synchronous rejections (`CATALOG_NOT_FOUND`, `PROTOCOL_INVALID_REQUEST`), as are an occupied id (`SERVER_ID_EXISTS`) and an unreachable catalog (`CATALOG_UNAVAILABLE`). A numeric `build` on a `fabric-meta` entry is `PROTOCOL_INVALID_REQUEST` — the wrong family's dialect. The job then: stamps the template → downloads and verifies the jar (byte progress, cancellable; Fill downloads verify a published sha256, Fabric downloads carry no published checksum and report the sha256 of what arrived) → writes the per-server config (`jar`, `mcVersion`, `javaMajorRequired`, `port`, optional `javaPath`) → registers last, the point of no return. **A failed or cancelled creation leaves nothing behind** — the instance directory is removed; the server appears in `server.list` (and via the `registered` event) only on success.
- Created servers live under the daemon's data dir (`instances/<serverId>`); `<data>/servers/<serverId>` remains the daemon's private runtime state and is never a server root. Clients reference the server by id only, as everywhere else.
- Checksums: a mismatch discards the download and fails the job with `CHECKSUM_MISMATCH` (`{expected, actual}` in context). Catalog outages answer `CATALOG_UNAVAILABLE`; a known name with nothing behind it answers `CATALOG_NOT_FOUND`.

### 7c. Java runtimes (Phase 6)

- `java.list` → `{runtimes: [{path, major, versionString, vendor, managed}]}` — every runtime the daemon could start a server with: `PATH`/`JAVA_HOME`/platform install roots plus the daemon's **managed** directory (`<data>/java`, `managed: true`). Every entry is *inspected* by running the JVM (`java -XshowSettings:properties -version`) and parsed — directory names prove nothing (ARCH-REVIEW §7). Inspection is cached per `(path, mtime, size)`: the JVM is asked once per unchanged file, never on UI refresh. A candidate that cannot be inspected is skipped, never listed.
- `java.install {requestId, majorVersion}` returns the running `Job` (`kind: "java.install"`): the daemon asks the Adoptium API v3 for the newest Temurin GA JDK for this machine's OS/architecture, fetches its published sha256, downloads and verifies the archive (byte progress, cancellable), extracts it into `<data>/java/<release>/`, and inspects the found `java` before reporting success. Extraction enforces the ADR-0009 trap list in miniature (regular files and directories only, no absolute/`..` names, one common top-level directory, entry-count and total-size caps). Installing an already-present release is idempotent (inspect-and-report). The Adoptium base URL is a daemon flag (`--adoptium-url`).
- Auto-selection (no explicit `javaPath`): the daemon picks the first inspectable candidate **satisfying the server's required Java major** — system candidates first, managed runtimes after. Nobody satisfies a stated requirement → `JAVA_NOT_FOUND` (no candidates) or `JAVA_INCOMPATIBLE` (all too old); both carry `install_java` remediation, which the UI turns into the `java.install` affordance. An explicit `javaPath` is authoritative and may fail the preflight honestly.

### 7d. Plugin catalog (ADR-0012)

- `plugins.search {serverId, query, limit?}` → `{target, hits: [{projectId, slug, title, description, downloads, iconUrl?, loaders}]}` — the Modrinth catalog, loader-faceted by the server's own layout: a `mods/` directory in the server root means the mods family (fabric, quilt, forge, neoforge), everything else is the Bukkit family (paper, spigot, bukkit, purpur, folia). The `target` rides in every answer so clients display where installs land. There is no game-version facet: the daemon does not know a registered server's Minecraft version, and it does not pretend to.
- `plugins.versions {serverId, projectId}` → `{target, versions: [{id, versionNumber, gameVersions, loaders, datePublished?, fileName?, sizeBytes?}]}` — loader-filtered, newest first. Versions whose primary file publishes no sha512 surface with no `fileName` (not installable, never half-installed).
- `plugins.install {serverId, projectId, versionId?, replace?, retireFile?}` returns the running `Job` (`kind: "plugins.install"`). The version resolves **at request time** — an unknown project or a project with no installable version for this server's loader family is a typed `CATALOG_NOT_FOUND` before any job exists (the java.install rule). The update rule (ADR-0012): a target file that is absent installs as before; one whose sha512 matches the published digest short-circuits to success without a download (an idempotent re-install); one with different content answers `PLUGIN_EXISTS` (the file in context) **before any job** — and only with `replace: true` does the verified download land over the old file through the shared atomic discipline (staging → fsync → rename); the old bytes survive a failed download intact, and the target never holds a half-written jar. The retire step (ADR-0012's update recipe): `retireFile` names the installed file this request UPGRADES — after the new bytes land and verify it is removed, so a version bump that changes the published name leaves one jar, not two. Land-first order: a refused retire fails the job after the landing, saying exactly what landed; an already-absent file is a no-op; a retire of the landed name itself collapses before any job; unsafe names and root-escaping symlinks are typed refusals at request time.
- `plugins.installed {serverId}` → `{target, entries: [{fileName, sizeBytes, modifiedMs, symlinkOutside}]}` — the directory **is** the inventory; the daemon keeps no plugin state beyond the files. A symlink leaving the server root is listed (the operator should know) and flagged `symlinkOutside`.
- `plugins.delete {serverId, fileName}` — the wire's filename is a suggestion: it passes the sanitizer (no separators, no control bytes, no Windows reserved names, 255-byte cap) and the rooted filesystem's checks before the disk sees it. A unit result (`EmptyResult`).

- `plugins.updates {serverId}` → `{target, entries: [{fileName, status, projectId?, installedVersion?, latestVersion?, latestVersionId?}]}` — the update check (ADR-0012's update rule, read side). Each jar in the target directory is identified by its own sha512 — the disk's bytes answer "what is this jar?" with no shadow state — and the catalog is asked, fresh, which version carries those bytes and what it now publishes for that project. `status` is one of `up-to-date` (the digest matches the newest installable version's published digest), `update-available` (a newer or different-loader installable version exists; the entry carries the full recipe — `projectId`, both version numbers, and the `latestVersionId` pin that applies the update through `plugins.install` with `replace` and the row's own `fileName` as `retireFile`), or `unmanaged` (the catalog has no file with these bytes, or knows them but publishes nothing installable for this server's loader family — the honest "no update button", with whatever story the catalog did supply). Entries sort by file name. A direct request, the same trade as `plugins.search`: jars are few, round trips are two per recognized file. A missing directory answers an empty report.

### 7e. Schedules (ADR-0014)

- The daemon runs the clock: a named schedule is a **when** (`{"kind": "interval", "everySecs": N}` — a fixed interval while the daemon runs, re-anchored at daemon start so downtime never stacks firings; `{"kind": "daily", "at": "HH:MM"}`; `{"kind": "weekly", "weekdays": ["mon".."sun"], "at": "HH:MM"}` — times are the daemon's local clock, re-read every tick), a **then** (`{"kind": "restart"}`, `{"kind": "backup"}`, `{"kind": "command", "line": "..."}`), and `enabled`. The records live in the server's own daemon metadata (`schedules.json`, versioned, atomic); the disk record is the only state.
- `schedules.list {serverId}` → `{serverId, schedules: [{id, name, spec, action, enabled, createdMs, lastFiredMs?, nextRunMs?}]}` — the stored record plus the daemon's computed next-run hint (display only; firing decisions re-evaluate the spec every tick). A paused schedule carries no `nextRunMs`: a fire that cannot happen is not promised.
- `schedules.create {serverId, name, spec, action, enabled?}` → `{serverId, schedule}` — validation happens at the edge: trimmed non-empty names (≤ 80 chars), strict 24-hour `HH:MM`, non-empty valid weekdays, non-empty console lines (≤ 256 chars), `everySecs ≥ 1` on the wire (clients nudge 300+). A violation is typed `SCHEDULE_INVALID`; the store never learns garbage.
- `schedules.update {serverId, scheduleId, name?, spec?, action?, enabled?}` → `{serverId, schedule}` — absent fields keep their stored values; unknown ids are typed `SCHEDULE_NOT_FOUND` (a typo must not look like success). No restart and no cached timers: the clock re-reads the store every tick, so an update lands on the next tick.
- `schedules.delete {serverId, scheduleId}` — unit result; the same typed refusal for unknown ids.
- Firing is the daemon's business and rides the ordinary paths — the same restart verb, the same backup job, the same stdin — so events and audit look exactly like an operator's action. Policy: restart and command fire only while the server is Running (a schedule never switches a machine on); backup fires either way; a skipped fire does not advance `lastFiredMs`, so a calendar schedule retries within its minute and then waits for the next one. Missed firings are skipped, never replayed.

### 7f. Publish (ADR-0017)

- The founder's sixth slice, with the §43 AI changelog room deliberately off the wire (standing scope: the changelog is an operator-edited string; the panel's room is named and disabled). A publication packages a **selection** of a server's files — never the root: an empty include list is a structural `PUBLISH_NOTHING_SELECTED` refusal, not a fallback to "everything".
- `publish.config.get {serverId}` → `{selection, providerId, providerSettings, title, description, version, changelog}` — the §40 form. Absent config answers honest defaults (nothing selected, the `archive` provider). Rules are internally tagged: `{"kind": "folder", "path"}`, `{"kind": "file", "path"}`, `{"kind": "glob", "pattern"}` — server-root relative, `/`-separated, no `..`, no backslashes, `**` only as a whole segment. Excludes win over includes.
- `publish.config.set {serverId, config}` → the stored config — a full, validated replace: selection rules, provider known, settings accepted (unknown keys refused). `publish.providers.list` → `{providers: [{id, displayName, needsCredential, credentialEnvVar?, settings: [{key, description}]}]}` — the two honest built-ins today (`archive`, `local-dir`); marketplaces arrive as new providers, nothing hardcoded (§40).
- `publish.preview {serverId}` → `{serverId, config, files: [{path, status, size?, sha512?}], counts: {added, modified, removed, unchanged, changed}, scan: {findings, filesScanned, filesSkipped}, blockingCount, selectedFiles, selectedBytes, lastPublication?}` — the §42 diff (a sha512 merge against the publication record; mtimes never consulted) and the §44/§46 scan in one read-only answer. `status` is `added` / `modified` / `removed` / `unchanged`; a removed row keeps its published size and carries no digest. Findings carry a redacted excerpt (a key name or a masked preview) — never the secret itself (§47). DiscordSRV's config is treated as especially suspicious exactly when the daemon can honestly say the plugin is loaded and functioning (the server Running and the jar present).
- `publish.execute {serverId, confirmUnsafe?}` → `{job}` (`kind: "publish.execute"`), the §74 state machine: preparing → scanning → packaging → uploading, as progress (`unit: "stage"`), completing only after the provider answers and the publication record commits atomically. Refusals land BEFORE the job exists: nothing selected (`PUBLISH_NOTHING_SELECTED`), unreviewed findings above `low` (`PUBLISH_SECRETS_DETECTED`, context names the count and files, remediation names review / exclude-file / publish-anyway / cancel), a needed credential absent (`AUTH_REQUIRED` naming the `ZAMIN_PUBLISH_CREDENTIAL_<ID>` env var — §47: the daemon never stores credentials). `confirmUnsafe: true` is the §45 Publish Anyway: explicit on the wire, two-step in the panel, a flag in the CLI. The job re-runs resolve and scan on fresh disk; packaging is a deterministic zip (staging → fsync → rename, `zamin-publish.json` manifest inside, 20k-entry / 2 GiB caps, cancellable).
- `publish.state {serverId}` → `{serverId, lastPublication?, receipt?, packagePresent}` — what went out last, the provider's receipt, and whether the package file still sits on the daemon's disk. `publish.review.set {serverId, file, kind, reviewed}` → the fresh preview: the §46 false-positive mechanism (the `(file, kind)` pair persists; reviewed findings stop blocking but stay visible).
### 7g. Server configuration surfaces (ADR-0019)

The layered config model (ADR-0007) on the wire: global defaults under per-server overrides, with per-field provenance so a client never guesses where a value came from.

- `config.get {serverId}` → `{serverId, displayName, jar?, effective, provenance}` — `effective` is the layered view (`stopTimeoutSecs`, `startupTimeoutSecs`, `port?`, `minMemoryMb?`, `maxMemoryMb?`, `extraJvmArgs`, `javaPath?`, `mcVersion?`, `javaMajorRequired?`, `backupKeep`); `provenance` answers `global` / `custom` field-for-field. `jar` is per-server only; absent means the built-in `server.jar` applies. A corrupt config file is a typed error, never a silent reset to defaults.
- `config.set {serverId, displayName?, jar?, settings}` → the fresh `config.get` view. The patch is **tri-state**: an absent field keeps the current override, `null` clears it (the global default applies again), a value sets it. serde requires an explicit tri-state deserializer for this — a JSON `null` otherwise collapses into absence. `displayName` has no clear state (a server always has a name). An empty patch is `PROTOCOL_INVALID_REQUEST`. Validation precedes any disk write and names the field (`CONFIG_INVALID` + context `field`): port 1024–65534, memory 16 MiB–1 TiB, timeouts ≤ 24 h, retention ≤ 1000, Java major 8–100, min ≤ max checked on the post-patch file, the jar through the same relative-path rule the spawner and publish enforce. A running server is never interrupted: overrides are read at spawn time, so changes apply the next start. `config.set` is audited.
- `network.status {serverId}` → `{serverId, desiredPort?, propertiesPort?, bindAddress?, portAvailable?, conflicts}` (founder §37). `desiredPort` is the layered config value; `propertiesPort` is what `server.properties` names — the boot authority, read but never written by this method; `bindAddress` is `server-ip` (empty string = all interfaces). `portAvailable` is a bind-test of `desiredPort ?? propertiesPort` at answer time — a moment-in-time probe, never a guarantee (the final authority is the server binding at boot); `null` when no port is known. `conflicts` lists other registered servers whose desired port equals this one's; self never appears, and an empty list serializes absent.

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
- `files.copy {serverId, from, to}` → `{path, files, bytes}` (ADR-0021) — a file or a whole tree, root-contained. A pre-flight measure pass totals the source before the first byte moves: over budget (2 GiB/call) is `FS_COPY_TOO_LARGE`, an existing target is `FS_COPY_TARGET_EXISTS` (copies never overwrite), a symlink anywhere inside the tree is `FS_SYMLINK_REFUSED`, past the 32-level walk bound is `FS_TOO_DEEP`. A refusal lands before anything is at the target.
- `files.search {serverId, query, limit?}` → `{hits:[{path, kind, sizeBytes?, modifiedMs?}], truncated, scanned}` (ADR-0021) — a bounded name walk over the whole root: case-insensitive substring, hits sorted by path, the daemon's staging dir invisible, `scanned` honest about coverage, `truncated` true when the 200-hit cap (`limit` defaults 100) or the depth bound cut the walk. An empty query is `PROTOCOL_INVALID_REQUEST` — the listing is how one sees a directory.

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
