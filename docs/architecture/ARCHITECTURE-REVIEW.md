# ZaminPanel — Architecture Review

*Date: 2026-10-05 · Scope: pre-implementation review of the ZaminPanel / ZaminCLI / ZaminCore product plan (Windows + Linux first-class, macOS later, remote agent later).*

---

## 0. Verdict

The plan is directionally right: Core owns everything, clients are thin, Linux is day one, events instead of polling, restrained design language. Most of it survives review unchanged.

Three structural changes are needed **before any code is written**:

1. **ZaminCore must be a resident daemon process (`zamind`), not a library embedded in Panel.** The plan's own requirements (server survives Panel closing, rediscovery, attach/detach, headless Linux, remote agent) all force this, and the plan never commits to it.
2. **The protocol — not Core's Rust/whatever API — is the real product boundary.** Every remote-agent requirement becomes cheap or expensive depending on five protocol rules that must be set in version 0: transport-agnostic framing, server references by ID (never client-side paths), idempotent lifecycle commands, first-class long-running jobs, and a versioned handshake.
3. **Pick the stack now.** Recommended: Rust for Core + daemon + CLI, Tauri 2 (web UI) for Panel. Alternatives evaluated and rejected below.

Everything else in this document is detail in service of those three decisions plus the specific failure modes the plan doesn't yet name.

### Coverage map (the 18 requested review areas)

| # | Area | Section |
|---|------|---------|
| 1 | Repository/module structure | §1.4 |
| 2 | Core API / ownership boundaries | §1.2, §2 |
| 3 | Process supervision | §3 |
| 4 | Event/state model | §4 |
| 5 | Async/concurrency | §5 |
| 6 | Filesystem abstraction | §6 |
| 7 | Java/runtime discovery | §7 |
| 8 | Networking/ports | §8 |
| 9 | Logging/terminal | §9 |
| 10 | Metrics | §10 |
| 11 | Configuration model | §11 |
| 12 | Windows/Linux platform abstraction | §12 |
| 13 | Testing strategy | §13 |
| 14 | Remote-agent future | §14 |
| 15 | Likely performance bottlenecks | §15 |
| 16 | Likely scaling problems | §16 |
| 17 | Overengineering to reject | §17 |
| 18 | Under-abstraction that forces rewrites | §18 |

Deliverables: recommended architecture §1–§14, reject list §17 + §19, risks §20, must-decide §21, safe-to-defer §22, implementation order §23.

---

## 1. The central decision: process topology

### 1.1 The gap in the plan

The plan says "the UI never owns the server, ZaminCore owns the server" and also requires:

- Server keeps running when Panel closes (§57, §34)
- Rediscovery and re-attach on relaunch (§34)
- Headless Linux with CLI only (§18–19 of the Linux plan)
- Later: remote machines via an agent (§38)

If Core is a library loaded inside the Panel process, the first requirement is violated: closing Panel destroys the supervisor. The standard escape hatch — "transfer ownership to a spawned headless supervisor on exit" — is where bugs live (crash during transfer, upgrade mismatches between the two processes, partial adoption, double-supervision windows). The plan currently doesn't choose, which means the first version will drift into Panel-owns-everything and rediscovery becomes a cleanup hack.

### 1.2 Recommendation: daemon-first, always

Core ships as a resident headless process: **`zamind`**. Panel and CLI are both protocol clients. Exactly one daemon per user, per machine.

```
┌──────────────┐   ┌───────────┐
│ ZaminPanel   │   │ ZaminCLI  │        thin clients
└──────┬───────┘   └─────┬─────┘
       │   JSON-RPC over IPC (named pipe / UDS)
       └────────┬────────┘
         ┌──────▼───────┐
         │   zamind     │   owns: registry, config, supervisor actors,
         │  (ZaminCore) │   ports, java, logs, metrics, backups, jobs
         └──────┬───────┘
       ┌────────┼─────────┐
       ▼        ▼         ▼
   java (MC) java (MC)   …   one actor per server
```

What this buys, concretely:

- **Detached servers are the default case**, not a special mode. Panel closing is just a client disconnecting. Rediscovery becomes "daemon already knows; reconnect."
- **Headless Linux is free**: `zamind` + `zamin` over SSH is the same product, not a fork.
- **Remote agent later = same protocol, different transport** (UDS/pipe → TLS socket + auth). No Core rewrite.
- **Multiple simultaneous clients are supported by construction** (Panel + CLI + future second client attach to the same truth).
- **One integration path.** No library-mode / daemon-mode behavioral drift, which is exactly the "five subtly different implementations" failure the plan bans.

Client UX implications to design now:

- Panel/CLI **ensure** the daemon is running (spawn it if not, with a version handshake). This must be invisible — double-clicking ZaminPanel must never show a daemon error.
- Daemon idles down per policy (stay alive while any server runs; optional "keep alive" setting; default exit when idle and nothing is supervised — or keep resident, configurable, and decide by feel in Phase 1).
- Single-instance enforcement: named mutex (Windows) / lockfile with liveness check in `XDG_RUNTIME_DIR` (Linux).

The honest cost of the daemon: its own lifecycle becomes a correctness surface (see risks, §20.1). That cost is lower than the alternative.

### 1.3 Ownership rules that follow

- Only the daemon spawns/owns/kills OS processes. Clients send commands; stdin lines are messages, serialized through the server's actor (single-writer).
- All persistent state (registry, config, job status, log rings) lives in the daemon and on disk under it — never in a client.
- Clients store only *view* state (window layout, open tabs, filter settings) locally.
- The daemon must survive itself: after a daemon crash/restart, it must **adopt** servers whose JVMs are still alive (§3.5), not respawn them.

### 1.4 Repository structure (revised)

The plan's `packages/{protocol,ui,shared}` is vague, and `shared` is a junk drawer waiting to happen. Trim to:

```
ZaminPanel/
├── Cargo.toml                     # workspace
├── crates/
│   ├── zamin-protocol/            # message types, envelope, versioning. No OS deps, no I/O.
│   ├── zamin-ipc/                 # framing + transport (named pipe / UDS), client + server sides
│   ├── zamin-core/                # the engine library
│   │   └── src/
│   │       ├── platform/          # ONLY place with #[cfg(windows)] / #[cfg(unix)]
│   │       │   ├── mod.rs         # traits + platform factory
│   │       │   ├── windows/
│   │       │   └── linux/
│   │       ├── server/            # identity, registry, config model
│   │       ├── supervisor/        # actors, state machine, adoption
│   │       ├── java/              # discovery, inspection, compatibility
│   │       ├── net/               # port management, server list ping
│   │       ├── fsops/             # rooted filesystem ops, archives
│   │       ├── logs/              # parser, ring buffers, crash context
│   │       ├── metrics/
│   │       ├── backup/
│   │       └── jobs/
│   ├── zamind/                    # daemon binary: hosts core + IPC server
│   ├── zamin/                     # CLI binary
│   └── testing/fake-mc-server/    # deterministic fake server for tests (§13)
├── apps/
│   └── panel/                     # Tauri 2 app: thin Rust host + web UI
│       ├── src-tauri/             # ~thin: bridges webview ⇄ daemon IPC
│       └── src/                   # design system + UI (TypeScript)
├── docs/
│   ├── adr/                       # architecture decision records — start here
│   ├── architecture/
│   └── development/
├── scripts/
└── .github/workflows/             # Windows + Linux CI from day one (§12.3)
```

Rules worth enforcing:

- `zamin-protocol` depends on nothing OS-specific; it must compile everywhere untouched. It is the future remote boundary.
- `zamin-ipc` holds framing and transport (client and server sides) so pipe/UDS code is written once. `zamind`, the CLI, and the Panel host depend on it.
- Only `zamind` depends on `zamin-core`. Clients know the protocol, never Core's internal API.
- Panel's Rust host contains **zero business logic** — it is a bridge. If a feature needs logic in the host, the logic belongs in Core.
- One repo, one workspace, one CI pipeline (the plan's "don't split Core into its own repo yet" is right).

### 1.5 Stack recommendation

| Option | Verdict | Why |
|---|---|---|
| **Rust core + Tauri 2 panel + Rust CLI** | **Recommended** | Meets every hard requirement: memory footprint, no runtime dependency for the manager itself, real async (tokio), single language across protocol/core/daemon/CLI; web UI gives the design ambition (motion, virtualized terminal) a mature toolkit (xterm.js, CodeMirror). Costs: slower iteration than TS, learning curve, WebKitGTK quirks on Linux (testable, known). |
| C#/.NET + Avalonia | Viable fallback | Avalonia is genuinely good on both OSes; but two runtimes (UI + none for daemon… actually one), NativeAOT edge cases with process APIs, weaker terminal/editor story. Fine choice if Rust feels wrong. |
| Kotlin/JVM + Compose Multiplatform | Reject | Ironic dependency: "install Java to manage Java." Bootstrapping problem, heavy daemon, jpackage packaging pain on Linux. Coroutines are nice; not worth it. |
| Go + anything | Reject for this product | Superb daemon/CLI, but no GUI path that reaches the plan's design bar (Fyne won't). Would force an Electron-ish split anyway. |
| Electron + TS everywhere | Reject | Heaviest memory story of all, and "optimized from day one… memory use" is a hard requirement. |

Terminal/editor components: **xterm.js** (WebGL renderer, virtualized by design) and **CodeMirror 6** (light, embeddable). Do not hand-roll a terminal emulator.

---

## 2. Core API & ownership boundaries

The plan lists subsystems but not the API's *shape*. Decisions needed at version 0:

1. **JSON-RPC 2.0 over framed messages.** Request/response with correlation IDs, plus server-initiated notifications for events. Human-debuggable, trivially versionable, zero codegen debt. (Reject gRPC/protobuf now — see §17.)
2. **Versioned handshake** as the first exchange: `{protocol_version, client, client_version}` → `{daemon_version, protocol_min/max}`. Mismatch produces an actionable error, never silence. This is what makes daemon/Panel/CLI upgrades survivable and remote possible.
3. **Servers are referenced by ID only.** Paths appear in protocol messages *only* as server-root-relative paths. If absolute client-side paths leak into commands, remote mode forces a protocol rewrite (§18).
4. **Idempotent lifecycle commands.** `start` on a STARTING server returns the current state (or a typed conflict error), never a second process. Every mutating request carries a client-generated request ID.
5. **Jobs are first-class.** Any long operation (create server = download JAR; backup; restore; later: software downloads) is a `job` object with progress events and cancellation. This is missing from the plan entirely, and retrofitting progress/cancel into an RPC API later is painful. Job events: `job.started / job.progress / job.completed{status, error}`.
6. **Structured errors**: `{code, message, context, remediation[]}` — machine codes so the UI can render "The server directory is not writable by the current user" with a Repair/Check Java button, not "Failed to start." Error taxonomy is a Core deliverable, not a UI afterthought.
7. **Subscription model**: clients subscribe per stream (logs, metrics, events) with a cursor; on reconnect: snapshot + replay-from-cursor (§4).

---

## 3. Process supervision

### 3.1 Actor-per-server

One actor (tokio task) per server, owning: the state machine, the child process handle, the stdin writer, the log pipeline, the metrics sampler handle. Commands are messages to the actor; events are messages out. No shared mutable state, no global locks, no `Arc<Mutex<…>>` soup. This single choice eliminates the worst concurrency bugs (double-start races, interleaved stdin, state torn between threads) and makes per-server isolation obvious.

### 3.2 State machine (revised)

The plan's seven states + attachment axis is right. Add three refinements:

```
register ──▶ NOT_RUNNING ──start()──▶ STARTING ──┬── "Done" / port listening ──▶ RUNNING
                 ▲                               │
                 │                     preflight fail (typed)
                 │                               ▼
                 │                        FAILED_PREFLIGHT
                 │
   STARTING ── exit before validation ──▶ CRASHED { phase: startup }
   RUNNING  ── unexpected exit ─────────▶ CRASHED { phase: runtime }
   RUNNING  ── stop() ──▶ STOPPING ── exited ──▶ STOPPED
   daemon restart ──▶ ADOPTING ── pid verified ──▶ RUNNING
                              └─ pid gone ──▶ STOPPED / CRASHED (from evidence)
```

- **FAILED_PREFLIGHT ≠ CRASHED.** Most "Minecraft won't start" is not a crash; it's a known precondition (§3.4). Typed preflight failures are what make the plan's error-design section (§33) real.
- **ADOPTING** as an explicit state keeps rediscovery honest instead of pretending the daemon "knows" a process it hasn't verified.
- Startup validation = log signature (`Done (…)! For help…` for Paper-family, pluggable per software) **or** port listening, within a configurable timeout. Timeout → stay STARTING with progress info; classify only on exit.

### 3.3 Shutdown ladder

Graceful shutdown goes through the console, per the plan — with the OS-level fallback ladder made explicit:

| Step | Linux | Windows |
|---|---|---|
| 1 | stdin: `stop` | stdin: `stop` |
| 2 | wait `stop_timeout` (default ~60s, configurable) | wait |
| 3 | SIGTERM to process group | CTRL_BREAK to process group (works because the daemon spawns the JVM with CREATE_NEW_PROCESS_GROUP and shares its console; verify in the first supervisor week — if console signaling proves unreliable on a headless daemon, the ladder degrades honestly to TerminateProcess) |
| 4 | wait ~10s | wait ~10s |
| 5 | SIGKILL to process group | TerminateProcess |

Windows note: there is no SIGTERM; TerminateProcess is SIGKILL. The CTRL_BREAK step is the only OS-graceful option — don't skip it.

### 3.4 Preflight (before spawn)

Typed checks, each individually reported: JAR exists/readable; Java selected and major version ≥ required; server dir writable by current user; EULA accepted (else typed `NEEDS_EULA` the UI turns into an accept dialog — classic first-run trap, cheap to catch); port free (§8); disk headroom sanity. Failing fast here converts the ugliest class of "crash" into a clear, fixable message.

### 3.5 Identity, adoption, and the PID-reuse trap

Storing a bare PID and "checking if it's alive" is unsafe on both platforms (PID reuse). The registry persists per server: `{pid, process_start_marker}` where the marker is creation time (Windows) or `/proc/<pid>` starttime + boot id (Linux). Adoption on daemon restart = PID alive **and** marker matches → adopt; alive but mismatched → treat as not ours, surface as UNKNOWN, never kill. This is a small amount of code that prevents a catastrophic class of bug (killing the wrong process). It should be in the first supervisor PR, not "hardened later."

### 3.6 Child-process tracking

Plugins can spawn children. Track the tree so "kill" really kills: **Job Object without KILL_ON_JOB_CLOSE** on Windows (kill-on-close would murder servers when the daemon dies — contrary to the detached requirement); **process group via setsid** on Linux, kill the group, not just the leader. This detail goes in `platform/`, where it belongs.

---

## 4. Event/state model

The plan's event list is good. Missing pieces:

1. **Per-subscriber bounded queues.** A global bus with one slow consumer must never backpressure the daemon. Each client subscription has its own bounded queue with a policy: state events are drop-oldest-with-marker; log lines coalesce with a `missed: N` marker (client shows "… N lines missed …" and can catch up via the file-backed log API).
2. **Cursor + snapshot reconnection.** Every stream carries a monotonic sequence. On (re)connect: `snapshot()` then subscribe-from-cursor. This is what makes Panel crashing and reattaching seamless, and it's the same mechanism remote will need over a lossy socket.
3. **Batched log delivery**: notifications flush at ~50 ms or N lines, whichever first. Line-by-line notifications over IPC would dominate CPU at busy-server rates for no benefit.
4. Events carry `server_id` and are *state transitions with reason*, not verbs: `state_changed{from, to, reason, exit_code?, error?}`. The plan's `server.starting/started/stopping/…` set is fine as sugar, but the canonical event is the transition.

---

## 5. Async/concurrency model

(Concretely, for the Rust recommendation; the shape transfers to any stack.)

- tokio everywhere in the daemon; **no blocking calls on async threads** — file I/O for jobs on `spawn_blocking`, git of it bounded.
- Bounded channels at every pipeline joint (spawn→log pipe→parser→rings→subscribers). Overflow policy explicit per joint, never unbounded memory "for now."
- One actor per server; daemon-level single-threaded registry; job runner with a small worker pool.
- Timeout on everything that waits on an external process (boot validation, stop ladder, archive ops).
- Graceful daemon shutdown: stop accepting clients → signal jobs to cancel → optionally stop supervised servers only on explicit user intent, never as a side effect.

---

## 6. Filesystem abstraction

Reject a generic VFS. Build exactly one abstraction: **a rooted, capability-checked server filesystem**.

- Every operation resolves and canonicalizes against the server root; anything escaping the root is denied with a typed error. Symlinks pointing out of the root: deny by default with a clear indicator in the file manager (opt-in allow for power users). The plan's "server root containment" is right; this is its implementation.
- Windows long paths (`\\?\`-prefixed handles internally, `longPathAware` manifest) — world folders blow past 260 chars routinely. Do this in the first fs commit, not as a bug report later.
- Atomic writes for config saves: temp file + rename (+ fsync on Linux). Corrupting `server.properties` on a power loss is unforgivable and cheap to prevent.
- Restore-from-archive traps: **zip-slip** path traversal checks; Windows-reserved names (`CON`, `NUL`, `COM1`…) when restoring a Linux-made backup on Windows; case-insensitive collisions. These are real cross-platform data-loss vectors and belong in `fsops/` with tests.
- Watchers: `notify` crate, but debounce aggressively and **never watch `world/` recursively by default** (region-file churn = event storms).

---

## 7. Java discovery

- Enumerate candidates per platform: `PATH`, `JAVA_HOME`; Windows: registry + `Program Files\Java`, `Eclipse Adoptium`, Zulu, Corretto, Microsoft JDK; Linux: `/usr/lib/jvm/*`, `/opt/*`, SDKMAN (`~/.sdkman/candidates/java`), asdf, jabba, nix profile, snap.
- **Never trust directory names.** Inspect each candidate by running `java -XshowSettings:properties -version` and parse stderr for real version/vendor/arch. Cache results keyed by path + mtime.
- Compatibility = data table (MC version → required Java major), vendor-agnostic, user-overridable with a visible "custom value" marker (matches the plan's settings provenance rule).
- The daemon needs no Java to run itself — that's a stack-level win (§1.5) worth keeping visible in docs.
- Defer: downloading JDKs (Adoptium API) to Phase 6+.

---

## 8. Networking & ports

- Availability check = bind-test the requested interface:port, then release. TOCTOU is acceptable (final authority is the server binding at boot; a race produces a typed boot failure with the port named).
- Distinguish *why* a port is busy: bound by another **managed** server (registry knows — offer "stop that server" / pick another) vs. foreign process (offer auto-select from the configured range). The existing PowerShell launcher's model — desired port in Core config, actual port in `server.properties`, explicit sync with consent, never silent — is correct; port it as-is conceptually.
- Server List Ping (TCP 47 handshake) in Core for players/motd/latency; it's also the seed of remote health checks. Vanilla+Paper compatible, no plugins required.
- **Honest TPS note (feeds §10):** TPS is not exposed by vanilla or via ping. Without a plugin, TPS comes only from log parsing (Paper logs it on demand / on warnings). Plan V1 metrics accordingly and don't promise a TPS number the engine can't measure.

---

## 9. Logging & terminal architecture

The single most performance-critical pipeline. Shape:

```
stdout/stderr pipes (per server actor)
      ▼  bounded channel
line splitter (bytes → lines, ANSI-aware)
      ▼
parser: [HH:MM:SS] [thread/LEVEL]: msg  → {level, thread, message, ts}
      ▼                     │
ring buffer (bounded,      ▼
~5k lines/server)   state-intelligence module
      ▼             (Done / stopping / errors / "Can't keep up" / crash sigs)
subscribers          │
(fans out in      state machine, crash context, metrics hints
batches ~50ms)
```

- **Terminal = live stream only** (ring buffer + subscribe). **Log viewer = the server's own files** (`logs/latest.log`, rotated archives) read incrementally, never re-ingested into memory wholesale. Core does **not** duplicate file logging — the existing launcher's "clear logs on start" behavior should be archived-only, never cleared (that's data loss disguised as housekeeping).
- Crash context = tail of `latest.log` + exit metadata → a typed `CrashReport{phase, exit_code, last_errors[], likely_causes[]}` feeding the plan's §33 UI.
- Frontend: xterm.js + WebGL renderer; input line-buffered with history; `!` launcher-command namespace intercepted client-side (same grammar as the PowerShell launcher — that UX earned its keep); renderer updates batched per animation frame, never per line.
- Autocomplete: static known-commands list V1. Real tab-completion requires server-side support (plugins/mods) — defer honestly.

---

## 10. Metrics

Sampler in the daemon at 1 Hz per running server — bounded ring (e.g., 3,600 points = 1 h), broadcast to subscribers at 1 Hz, history via RPC:

| Metric | Source | Notes |
|---|---|---|
| CPU % | process CPU-time deltas (`/proc` or `GetProcessTimes`) | reliable, cheap |
| Memory | RSS (Linux) / commit (Windows) of the JVM process | reliable |
| Players | Server List Ping (5–10 s) + log join/leave events | reliable |
| Uptime | actor state | trivial |
| TPS | log parsing only | **not available without a plugin** — surface honestly, add optional metrics plugin later |
| World size | periodic (minutes) async walk with budget | cheap enough, defer to Phase 4+ |

That's the entire V1 metrics subsystem: one sampler, one ring, one notification type. Anything richer (GC pauses, chunk ticks) belongs behind a small agent plugin later, not in Core.

---

## 11. Configuration model

- **Formats**: TOML for human-edited config (comments matter), JSON for machine state (registry, job status). Versioned files (`version = 1`) with migrators from the first release — config migration retrofits are miserable.
- **Layering**: global defaults in app-data, per-server config layered over it, and every field carries provenance (`global` | `custom`) so settings can *show* "Using global default" vs "Custom value" — the plan's §21 rule, made structural.
- **Registry & rediscovery**: registry (app-data) holds `{id, name, path, config…}`; each server root carries a tiny marker file (`.zamin/server.json` with id + schema version) inside it. Rediscovery = registry entries verified (path exists, marker matches) + optional marker-scan of configured roots for entries the registry lost. This generalizes the existing launcher's `active-servers.json` idea.
- **server.properties**: typed structured editor for known keys + raw editor always available (plan agrees); writes go through the atomic-write path; desired-port sync is explicit (§8).
- App data placement: `dirs`-crate conventions — `%APPDATA%`-family on Windows, `XDG_CONFIG_HOME` / `XDG_STATE_HOME` / `XDG_DATA_HOME` on Linux. Server roots are always user-chosen and never inside app data (plan already separates these — keep it).

---

## 12. Windows/Linux platform abstraction

### 12.1 The seam inventory (complete list of what belongs in `platform/`)

| Concern | Windows | Linux |
|---|---|---|
| Spawn | argv array, never a shell | argv array, `setsid` + own process group |
| Graceful OS stop | CTRL_BREAK to group | SIGTERM to group |
| Forced kill | TerminateProcess | SIGKILL to group |
| Child tracking | Job Object (no kill-on-close) | process group |
| PID identity | pid + creation time | pid + starttime + boot id |
| IPC transport | named pipe, current-user ACL | UDS in `XDG_RUNTIME_DIR` (0700) |
| Single instance | named mutex | lockfile + liveness |
| App dirs | Known Folders | XDG base dirs |
| Java discovery | registry + Program Files + env | /usr/lib/jvm, /opt, sdkman, asdf, env |
| Long paths | `\\?\` + manifest | n/a |
| Permissions surfaced | ACL-based errors | mode-bit errors ("dir not writable by current user") |
| Autostart (later) | Task Scheduler / Run key | systemd user unit |
| Notifications (later) | toast | freedesktop Notifications |

Everything else in Core is platform-neutral. `#[cfg(windows)]` / `#[cfg(unix)]` may appear **only** under `platform/` — enforced by review convention plus a workspace lint (`#![ forbid ]`-style hygiene via clippy groups; a tiny custom lint or codeowner rule is fine at this size).

### 12.2 Linux genuineness

The Linux paste's rules are all correct and adopted: no root for normal operation, XDG everywhere, `.desktop` integration, no distro-`ifdef`s (care about kernel/desktop/runtime, not `/etc/os-release`), baseline = Ubuntu/Debian/Fedora/Arch/Mint/openSUSE, test GNOME + KDE, Wayland + X11. Two refinements:

- **Tauri/WebKitGTK on Linux is the least-deterministic part of the whole stack** (input methods, fractional scaling, compositor quirks). Budget real testing time; this is the most likely source of "feels Windows-ported" bugs.
- Packaging: AppImage + portable tar.gz first; `.deb`/`.rpm` once stable; Flatpak only when the sandbox/permission story is deliberate.

### 12.3 The enforcement mechanism that actually keeps Linux first-class

**CI on both OSes from day one.** GitHub Actions matrix `windows-latest` + `ubuntu-latest`, running build + clippy + tests on every push. You cannot accidentally rot a platform that always compiles and always runs its tests. Locally, WSL2 covers day-to-day Linux verification. This is the single cheapest guarantee in the whole plan and it must exist before Phase 1, not after.

---

## 13. Testing strategy

- **`fake-mc-server`** (crates/testing/): a tiny deterministic binary that mimics a Paper lifecycle — configurable boot time, boot failure, exit codes, crash mid-run, ignores `stop`, slow stop, stdout flood modes. It gives ~90% of lifecycle testing with zero Java in CI, on both OSes, in milliseconds. This is the highest-leverage test investment in the project.
- **Golden logs**: commit anonymized real-world log samples (Paper, Spigot, Folia, startup-flood, crash traces) as parser fixtures. The log parser is load-bearing (state machine + metrics + crash context) and it must be tested against real mess, not idealized strings.
- **Lifecycle matrix** against the fake server: start/stop/restart × clean/crash/timeout/hang, duplicate-start, adopt-while-running, adopt-foreign-pid, kill-tree, preflight failures, port conflicts.
- **Protocol conformance fixtures**: recorded message exchanges asserted version-over-version, so breaking changes are caught before clients ship.
- **Performance budgets in CI** (simple timing asserts, no fancy harness): log ingest ≥ 20k lines/s; 20k-file directory listing < 200 ms; daemon RSS with 5 running fake servers < 100 MB; IPC round-trip p99 < 5 ms.
- **UI tests**: component tests + screenshot states for the design-critical surfaces; full E2E via tauri-driver later (flagged: historically flaky — keep optional).
- The plan's "test where it hurts" list (§48) is good — encode it as the perf-budget suite.

---

## 14. Remote-agent future

With daemon-first topology, remote becomes additive rather than a rewrite. What must be true **now** (all already stated, collected here):

1. Protocol is transport-agnostic framing (JSON-RPC over anything that carries framed bytes).
2. Server references are IDs; paths are server-root-relative (§2.3).
3. Versioned handshake + typed errors (§2.2, §2.6).
4. Idempotent commands + request IDs (§2.4).
5. File operations stream in chunks with hashes; no "read whole file into one JSON message."
6. Auth is a hook in the connection path from day one — local mode = "OS user + pipe/UDS ACL is the credential" (documented, not ignored).
7. Jobs are cancellable, resumable-ish objects (§2.5).

What stays deferred: TLS, tokens, per-operation ACLs, audit log, agent install story. Zero scaffolding code for these beyond the hooks above — anything more is premature (§17).

---

## 15. Likely performance bottlenecks (and their fixes, so they're designed in)

1. **Log flood**: parse once in the daemon, batch notifications (~50 ms), ring buffers bounded; frontend renders per frame, not per line. A 20k-lines/s burst must cost the daemon single-digit % CPU and the UI nothing but a scrollbar.
2. **World directories**: never recursive-walk on the UI path; listing is depth-1 + lazy expansion; watchers exclude `world/`.
3. **IPC serialization**: chunked streams for files; JSON fine for control plane; MessagePack only if measurements demand it.
4. **Metrics**: one 1 Hz sampler per server, preallocated rings — no allocation churn, no per-sample messages unless subscribed.
5. **UI threads**: all daemon I/O off the render path (bridge forwards via events); no synchronous IPC in the webview; virtualized lists everywhere (files, logs, servers).
6. **Java discovery**: cache inspection results; never re-`XshowSettings` on UI refresh.
7. **Startup storms**: on Panel open, snapshot + one burst of state — coalesce server-state events within the first second.

---

## 16. Likely scaling problems (eventual, designed-around-now)

1. **Daemon upgrades while servers run** → adoption path (§3.5) must be rock solid; document "daemon may be restarted; servers survive" as an invariant with tests.
2. **Multiple daemons** (different users on one machine) → per-user socket namespace; fine by construction; just don't write a global singleton lock.
3. **Protocol drift** between daemon and clients in the wild → handshake + CI fixtures (§13).
4. **Log index growth** for the log viewer → file-backed incremental reads V1; SQLite full-text index later if search becomes slow. Don't build the index now.
5. **Backup storage growth** → retention policy is a required feature (Phase 5), not an option; disk-full during backup must be a handled job failure.
6. **20+ servers in the UI** → virtualized lists, tab overflow behavior — UI-side, cheap if planned.
7. **Big worlds** (10⁵ files) → fs ops all budgeted and paginated (§15.2).

---

## 17. Overengineering to reject (explicit list)

1. **gRPC/protobuf codegen** for IPC — framed JSON-RPC is debuggable with zero toolchain; binary framing is a measured later step.
2. **A generic VFS abstraction** — the rooted server filesystem is the only fs abstraction needed (§6).
3. **Event sourcing / full audit persistence** — rings + snapshots + versioned state files suffice.
4. **SQLite on day one** — files first; a database arrives when a query actually needs it (log index, §16.4).
5. **A plugin/extension system for Core itself** — defer indefinitely; design boundaries (§2) keep the door open, build nothing.
6. **Multi-language Core bindings / public SDK** — the protocol is the SDK; publish it when remote exists.
7. **macOS scaffolding** — platform traits stay macOS-ignorant; no `macos/` dirs until there's a target.
8. **A trait zoo for software sources** — Paper/Purpur/Folia in V1 is data (catalog + URL templates), not an inheritance hierarchy; introduce a tiny `SoftwareSource` only when the second family (Fabric/Forge) actually lands.
9. **Custom terminal emulator / custom editor** — xterm.js + CodeMirror.
10. **Container adapters (Docker/Podman) plumbing** — the platform seam keeps it possible; build nothing until there's a user.
11. **"packages/shared"** — delete the concept; if two crates need a type, it lives in `zamin-protocol` or in one of them.

---

## 18. Under-abstraction that would force rewrites (the complement)

1. Absolute paths in protocol commands (breaks remote).
2. PID-only process identity (breaks safe adoption/kill).
3. Panel-embedded supervision (breaks detach/rediscovery — the daemon decision, restated).
4. State held in clients (breaks multi-client and remote).
5. Stringly-typed errors over IPC (breaks every remediation UX in the plan's §33).
6. Unversioned config/state files (breaks upgrades silently).
7. stdout capture coupled to a specific UI (breaks late-subscriber attach — ring + cursor is the fix, §4/§9).
8. Windows-only path/permission handling baked into fsops (breaks restore-across-platforms, §6).
9. No job/cancel concept (breaks downloads/backups UX retrofit).
10. Time handling without explicit timezone/epoch discipline in logs and metrics (breaks every "started 3h ago" and cross-day log correlation).

---

## 19. Dispositions on the existing plan documents

**Endorsed as-is**: product philosophy; tab/workspace model; New Server flow; terminal-vs-logs split; error design; notifications taxonomy; motion tokens; dark/light layer model; command palette; accessibility incl. reduced motion; single-accent restraint; "polish continuously"; 0.1.x release honesty; vertical-slice-first.

**Amended**:
- Windows-first → Windows + Linux day one (the Linux paste — adopted, with the two refinements in §12.2).
- "Beautiful shell in Phase 2, workspace in Phase 3" → merge: minimal shell + one perfect vertical slice together; beauty accrues continuously (the plan's own §56 already argues this).
- Phases renumbered so **CI + skeleton are Phase 0** (§23).

**Ported from the PowerShell launcher** (concepts, not code — a 3.6k-line PowerShell REPL doesn't translate into a daemon world): the `!` command grammar; crash-menu UX (Restart / Start different / Main menu) → becomes the crash card; `active-servers.json` recovery → registry + marker + adoption; port auto-select + explicit properties sync (§8); per-ID/alias server resolution. Its "clear logs on start" behavior is deliberately **not** ported.

---

## 20. Biggest technical risks (ranked)

1. **Daemon lifecycle correctness** — adoption, crash-of-daemon, upgrade handshake, single-instance races. Highest new-correctness surface the plan introduces; mitigated by the lifecycle matrix (§13) and adoption-in-first-PR rule (§3.5).
2. **WebKitGTK/Linux desktop variance** — the UI's least controllable dependency; mitigate with early Wayland/X11 + GNOME/KDE smoke tests in Phase 3, not Phase 6.
3. **Log-format drift across MC versions/forks** — parser is load-bearing; mitigate with golden fixtures + tolerant parsing + software-specific signatures as data.
4. **Scope vs. one maintainer** — the design bar is high; the phased order (§23) plus the reject list (§17) is the mitigation. The vertical slice is the product; everything else is scheduled behind it.
5. **Live-world backup consistency** — `save-off`/`save-all flush` windows, partial-copy failures; Phase 5 with job infrastructure and tests, never a naive folder copy.
6. **TPS expectations** — surfaced honestly (§8/§10) so the UI never shows a fabricated number.

---

## 21. Must decide before implementation

1. Stack (recommendation: §1.5 — Rust + Tauri; needs explicit sign-off).
2. Daemon topology + daemon idle/autostart policy (§1).
3. Protocol v0: envelope, handshake, error taxonomy, job model, subscription/cursor semantics (§2, §4) — write it as a doc + `zamin-protocol` types before any server code.
4. Server identity & registry layout + marker file schema (§11).
5. State machine incl. preflight taxonomy and crash classification (§3).
6. Config layering & provenance rules (§11).
7. License (MIT/Apache-2.0 vs GPL — affects the future marketplace/plugins story; cheap to decide now).
8. CI matrix + minimum review bar (clippy strict, fmt, deny) — the "human-maintained feel" is enforced here more than anywhere.

Each becomes one short ADR in `docs/adr/` — that practice itself is the strongest "not vibe-coded" signal available.

## 22. Safe to defer

JDK download management; structured config editors beyond `server.properties`; plugin install/update from catalogs; SQLite log index; systemd/Windows-service modes; remote agent auth/TLS; Flatpak; macOS; container adapters; team/fleet features; accent customization; CLI interactive REPL (subcommands + `attach` first).

## 23. Implementation order (revised)

| Phase | Deliverable | Proof of done |
|---|---|---|
| **0 — Foundation** | Decisions above → ADRs; monorepo skeleton; **CI green on Windows + Linux**; `zamin-protocol` v0; `platform/` seams; `fake-mc-server` | Both CI lanes green; ADRs merged |
| **1 — Core** | Registry + config; supervisor actors; full lifecycle matrix vs fake server; preflight; events; JSON-RPC over pipe/UDS; adoption | `zamind` runs, lifecycle tests pass on both OSes |
| **2 — CLI** | `zamin` list/status/start/stop/restart/logs -f/attach + `--json` | Second client validates the protocol end-to-end; usable over SSH |
| **3 — Panel slice** | Tauri shell (tabs, sidebar, palette) **and** the full vertical slice: New Server → start → terminal → stop → crash card | The §53 journey feels right; Wayland/X11 + GNOME/KDE smoke-tested |
| **4 — Workspace** | Logs page, files, editor, players, performance | Plan §48 horrible-case tests pass |
| **5 — Backups** | Backup/restore jobs, retention, crash recovery UX | Restore-from-Linux-on-Windows and vice-versa tested |
| **6 — Software & Java mgmt** | Catalog, downloads, JDK fetch, templates | New Server flow needs zero manual JAR handling |
| **7 — Packaging & integration** | Installer + portable (Win), AppImage + tar.gz (Linux), `.desktop`, notifications, autostart | Real installs on clean Win11 + Ubuntu/Arch VMs |
| **8 — Services & remote** | systemd user unit / Windows service; ZaminAgent (TLS + auth) | Headless box managed from a desktop Panel |

---

*Recommendation: ratify §1.1–1.3 (daemon), §1.5 (stack), and §2 (protocol rules) as the first three ADRs; everything else in this document is refinement that can land during Phase 0/1.*
