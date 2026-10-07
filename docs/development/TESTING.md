# ZaminPanel Testing Conventions

*Testing follows the architecture: the lifecycle matrix, the protocol, and the filesystem safety model are the load-bearing surfaces, so they are the best-tested.*

## Layers

| Layer | Where | What |
|---|---|---|
| Unit | `#[cfg(test)]` beside the code | state machine transitions (table-driven), config parsing/migration, log parser, port logic, path containment |
| Integration | `crates/*/tests/` | real filesystem, real processes (fake-mc-server), IPC client ⇄ daemon |
| Protocol conformance | `crates/zamin-protocol/tests/fixtures/` | recorded exchanges asserted across versions; breaking changes must update fixtures intentionally |
| CLI end-to-end | `crates/zamin-cli/tests/` | the real `zamin` binary drives the real `zamind` binary over the wire: register/start/stop, `logs -f` streaming, `attach` stdin round trip, `--json` error objects. Workspace binaries are located next to the test binary — run `cargo test --workspace`, not per-package tests |
| Performance | `#[ignore]`d tests run by the nightly CI job | budgets from [PERFORMANCE-BUDGETS.md](PERFORMANCE-BUDGETS.md) |
| UI | `apps/panel` — vitest: protocol client (scripted mock transport), stores, shell rendering | components render states (default/hover/focus/disabled/loading/error/selected); critical flows once a test harness exists |
| Panel smoke | `apps/panel` — manual browser loop | real daemon + `dev-bridge.mjs` + vite dev: handshake badge, snapshot-seeded sidebar, `zamin register` appears live, `start/stop` state dots move — the events stream as the only reconcile channel |

## fake-mc-server

`crates/testing/fake-mc-server` — a deterministic Rust binary that mimics a Paper lifecycle without Java. Modes (composable flags):

- `--boot-ms N` — startup duration
- `--fail-boot` — exit during startup (with `--exit-code N`)
- `--exit-after-boot` — die right after the `Done` line
- `--crash-mid-run` — exit while running
- `--exit-code N` — custom exit code
- `--ignore-stop` — stdin `stop` has no effect (forces ladder step 3)
- `--slow-stop-ms N` — shutdown takes N ms (forces timeouts)
- `--flood-stdout N` / `--flood-stderr N` — N lines/s forever
- `--flood-unbounded` — as fast as the process can emit (true capacity measurement)
- `--port N` — answer Server List Pings on the port from `server.properties`/this flag
- stdin `save-off` / `save-all` / `save-on` — Paper-faithful replies ("Turned off world auto-saving", "Saved the game"), so the live-backup save window is testable end to end

Lifecycle tests spawn it through the real supervisor and real IPC. No test may sleep-and-hope: state changes are awaited via events.

Like the real thing, the mimic owns `logs/latest.log` in its working directory (the server root): the file is created fresh at boot — Paper rotates the previous session away — and every emitted line lands in stdout and the file, flushed per line. "The log files hold the full history" (ADR-0006) therefore holds for mimicked servers exactly as it does for real ones; tests that need a crafted history write the file before starting anything.

## Lifecycle matrix (required coverage)

start · stop · restart · crash during startup · crash while running · stop timeout → ladder escalation · hang on stop · duplicate start · start during stopping · adoption after daemon restart · foreign PID in registry (never adopt, never kill) · kill tree (child of child) · each preflight failure · port conflict · daemon restart while running · client disconnect/reconnect with cursor replay.

## Backups (required coverage)

Roundtrip (create → manifest → list → tamper → restore → byte-identical) · zip-slip rejection · absolute/backslash/dot-component names · Windows reserved names per component, case-insensitively, extension or not · case-fold collisions · entry/size limits · link and device entries rejected · cancel between files (staging cleaned, root untouched) · commit failure rolls the live tree back · retention keeps the newest N · disk-full is a typed error · restore refused while running · live save window observed in the server's own log · job events (started/progress/completed) over the wire. These are the enforcement of ADR-0009 for archives — a missing one is a security regression.

## Software catalog & Java (required coverage)

The daemon's catalog base URLs are flags: `--catalog-url` (PaperMC Fill API v3) and `--adoptium-url` (Adoptium v3). Tests NEVER touch the live network — an in-process mock HTTP server speaks the recorded API shapes (see `crates/zamin-core/src/software/tests.rs` and the `MockHttp` in `crates/zamind/tests/common/mod.rs`). Required:

- Catalog browsing: versions flattened and sorted newest-first (final releases above their pre-releases, numerics compared numerically) · builds newest-first with only `server:default` downloads kept · the version's own Java requirement served API-first with the local table as fallback · unknown project/version → typed `CATALOG_NOT_FOUND`; transport failure → `CATALOG_UNAVAILABLE`.
- Downloads: streaming to a staging file → sha256 verified → atomic rename; progress and cancel checkpoints per chunk · checksum mismatch discards the bytes and fails typed (`CHECKSUM_MISMATCH`) · a cancelled or failed download leaves no partial artifact · an existing target is never overwritten.
- `server.create`: typed synchronous rejections before any job (unknown project/version/template, occupied id) · the happy path lands verified jar bytes + template stamps (`eula.txt` with `eula=false` — acceptance stays the user's click) + per-server config (`mcVersion`, `javaMajorRequired` from the catalog, `port` in settings AND the stamped `server.properties`) · failure or cancellation anywhere before registration removes the instance directory · the full journey: create → typed `NEEDS_EULA` on first start → accept through the file surface → running (zero manual JAR handling).
- JDK fetch: Adoptium asset + checksum-link parsing · extraction safety for both archive formats (regular files/directories only, no absolute/`..`/backslash names, one common top-level directory, entry/size caps, tar headers crafted byte-level exactly as a hostile producer would) · the fetched `bin/java` is inspected, never trusted by name; uninspectable runtimes are removed, not left to poison discovery · idempotent reinstall · managed runtimes appear in `java.list` (`managed: true`) and are considered by auto-selection AFTER system candidates · the auto-selection is requirement-aware (`JAVA_INCOMPATIBLE` when all candidates are too old, `JAVA_NOT_FOUND` on a bare machine).
- Zip fixtures are built with the same crate the extractor reads, so the Windows format is exercised on every platform; the inspect loop is `#[cfg(unix)]` because the fake `java` is a shell script.

## Packaging & integration (required coverage)

Everything here must pass on both CI lanes and, where filesystem state is involved, in a sandbox (never the real user HOME):

- Daemon bring-up: `resolve_daemon_binary` sibling-first-then-PATH, all cases (sibling found, PATH found, nothing found) · `endpoint_ready` probes a live bind as ready and an unbound endpoint as down (see `apps/panel/src-tauri/src/daemon_ensure.rs` tests, run in the bundle workflow's lanes).
- Autostart: XDG entry content and round-trip (absent → off, set on → on, set off → gone, removing nothing is fine, fresh directories created) against a sandboxed config home; the HKCU Run-key round-trip runs only on the Windows lane (see `apps/panel/src-tauri/src/autostart.rs`).
- Notifications taxonomy (pure, in `apps/panel/src/integration/notifications.test.ts`): hidden or blurred → notify; focused → silent · crash content names the server, the phase, and the exit code when known · job content distinguishes succeeded/failed/cancelled and resolves unknown kinds honestly.
- Integration seam (`apps/panel/src/integration/autostart.test.ts`): outside Tauri the toggle reports unavailable and refuses to pretend; inside Tauri it mirrors the host, maps an undeterminable state to unavailable, and stays honest on host refusal.
- Palette: the autostart command is hidden where the host cannot deliver it and labeled by the current state ("Start with the system" / "Stop starting with the system").
- Install script (`scripts/packaging/test-install-linux.sh`, runs in the bundle workflow): install (all four binaries) → Exec rewritten → icons → idempotent upgrade → autostart on/off → daemon and agent service units on/off (plus `systemd-analyze verify`) → broken-payload rejection → clean uninstall. Plus `shellcheck` on all packaging scripts and `desktop-file-validate` on the entry.
- Install script, Windows lane (`scripts/packaging/test-install-windows.ps1`): layout → Start Menu launcher → idempotent upgrade → logon-task toggles (a Task Scheduler refusal is an honest pass; the off toggle always cleans up) → typed errors → clean uninstall.
- The §23 proof of done — clean Win11 + Ubuntu/Arch VM installs — stays a documented manual step; `bundle.yml` exists so those VMs only ever install artifacts that already built, tested, and packaged green.

## Services & remote (required coverage)

The remote path is the security boundary of the product (ADR-0011); every rule below is enforced by a test that fails loudly when the rule breaks:

- TLS material: the self-signed certificate is generated once per install and reloaded identically (same fingerprint) · the key and token files are 0600 · a fresh install yields a different fingerprint · the printed fingerprint matches an independent SHA-256 of the certificate.
- The auth gate (before any local daemon connection exists): missing/empty `auth` → `AUTH_REQUIRED` · wrong token → `AUTH_REJECTED`, connection closed · first frame not a readable `daemon.hello` request → Null-id `PROTOCOL_INVALID_REQUEST` (never a guessed id) · first frame not `daemon.hello` at all → `PROTOCOL_VERSION_UNSUPPORTED`, mirroring the daemon's own rule · token comparison digests both sides first (fixed-length, constant-time).
- The relay: an authenticated hello round-trips and post-handshake frames relay both directions · the agent cannot reach its daemon → typed `DAEMON_UNREACHABLE` · one remote connection maps to one daemon session · either side closing ends the session.
- The transport rejects what it must: a plaintext peer never receives protocol data (at most a TLS alert) · a wrong pinned fingerprint fails the TLS handshake · `InsecureSkipVerify` works but is an explicit, documented mode.
- The CLI remote mode (deferred list, landed): the real `zamin` binary drives the real `zaminagent` + `zamind` chain — `--remote <addr> --fingerprint <hex> --token-file <path>` answers `daemon.status`, registers and lists through the relay · a wrong token is the typed `AUTH_REJECTED` · a wrong fingerprint fails the TLS handshake · omitting `--fingerprint` without the explicit insecure flag is a usage error.
- The audit log (deferred list, landed): `<data>/audit.log` appends one JSONL line per handshake and per mutating command, carrying the protocol client (name + version) and the daemon's outcome code · accepted and rejected hellos both land · `server.register/start/stop` are audited · reads (`server.list`) are not · a missing directory is a warning, never a panic.
- The panel side: connection profiles persist to localStorage (local default, add/remove/activate remotes, active-id fallback on remove) · `transportSpec` derives the bridge relay URL + hello credential per profile · the client sends `hello.auth` only when a credential is set, and `reconnect()` swaps the live transport and re-handshakes with the new credential without waiting for the retry schedule.
- Player roster (log-roster refinement): join/leave lines parse only with a legal username charset (a chat line saying the phrase never joins the roster) · joins/leaves over the real daemon end-to-end (fake-mc-server `join`/`leave` over stdin → pumps → hub → `players.list`) · the roster dies with the server process · the ping side stays its own honest shape.

## Metrics (required coverage)

The sampler is the only source of performance numbers; the panel never invents one:

- Platform sampling: a live process reports cumulative CPU time + RSS (`/proc` on Linux, `GetProcessTimes`/`GetProcessMemoryInfo` on Windows) · a missing PID answers `None`, never zeros · one read is cheap (asserted < 10 ms; measured microseconds).
- The 1 Hz actor sampler publishes while a process is live and stops with it · CPU% appears from the second sample of a process generation (no fake base) · RSS is measured on every platform · players come from the live roster (`null` until the server has logged anything) · `tps` stays `null` unless actually measured — it never is.
- Hub semantics: the per-server ring is bounded (600) and keeps the newest · a fresh metrics subscription receives the ring's latest sample first · a slow subscriber coalesces latest-wins (pending slot, flushed on the next publish; the stale samples are dropped without `Missed` markers) while an equally-slow `events` subscriber still gets its `Missed { N }` marker (both asserted side by side).
- `metrics.range` serves the ring chronologically, trims to the newest `maxSamples`, and answers typed `SERVER_NOT_FOUND` for an unknown server — all end-to-end over the wire while a fake server runs.
- Panel: the metrics store dedupes ring replay vs. live ticks by timestamp, caps the window, seeds range history under live data · header chips subscribe to the metrics store alone (a 1 Hz flood re-renders two chips, never the console — render-count assertion) · stale samples never display once the server leaves a live state.

## Performance budgets (required coverage)

The budgets in [PERFORMANCE-BUDGETS.md](PERFORMANCE-BUDGETS.md) are tests in
`crates/zamind/tests/perf.rs` (`#[ignore]`d; the nightly `perf` workflow runs
them with `--ignored`). Each assertion gates a published number:

- IPC round trip over the real daemon: p50 < 1 ms, p99 < 5 ms (2,000 warm samples).
- Cold start → accepting + speaking the protocol: p50 < 500 ms across 5 runs, max < 1 s.
- Sustained ingestion ≥ 20,000 lines/s over a 10 s window through the real pipeline (spawn → parse → ring → stream), daemon RSS under the flood < 150 MiB.
- Burst with a stalled subscriber: 14 s without reading under an unbounded flood → the daemon's `daemon.status` round trips stay sub-second throughout (the stdout reader never blocks on delivery), the resuming subscriber receives a `Missed { missed: N > 0 }` marker (no silent loss, ADR-0006), RSS stays under 150 MiB, and post-catch-up throughput stays above the sustained budget with batched delivery (avg ≥ 50 lines/notification).
- State change → events notification delivery: p99 < 20 ms.
- 20k-entry directory listing: p95 < 250 ms.
- Idle daemon RSS < 50 MiB (reference platform; Linux-only via /proc).
- Five servers streaming 1k lines/s each with an active subscriber: daemon RSS < 150 MiB.
- Terminal input echo, round trip via the daemon (stdin request → the server's reply line on the logs stream): p99 < 50 ms over 100 samples — the test that forced the pump's flush tick from 50 ms to 10 ms.
- Panel bundle budgets (`apps/panel/perf/budgets.mjs`, run after the panel build): entry chunk ≤ 90 KB gzip, any single chunk ≤ 90 KB, total JS ≤ 170 KB, total CSS ≤ 12 KB — the guard that keeps the cold-start payload honest after the xterm/modals code split.
- Deferred list (`apps/panel/src/ui/deferred.test.tsx`): a 2,000-entry collection commits its 120-row window synchronously, catches up 240 rows per idle frame, resets for free on fresh data (asserted mid-stream — no frame carries the full old window), keeps the grown window across no-new-data re-renders, and "Show all" is one explicit commit.

## Fixtures

- Golden logs under `crates/zamin-core/testdata/logs/<software>/`: real anonymized Paper, Spigot, Folia, Purpur samples — startup floods, warnings, crash traces. Parser changes must not alter golden output without an explicit diff review.
- Config migration fixtures: one file per historical schema version; CI migrates every fixture every release.

## Filesystem security tests (required)

Traversal (`..`, absolute, encoded), symlink escape, zip-slip, Windows reserved names, case-insensitive collisions, partial extraction rollback, disk-full, long paths (Windows runners), watcher exclusion of `world/`. These tests are the enforcement of ADR-0009 — skipping one is a security regression, not a coverage gap.

## Rules

- Deterministic or rejected: no sleeping against wall clocks, no order-dependent tests, time comes from an injectable clock where it matters.
- Test names state the behavior: `start_while_starting_returns_current_state`, not `test_start_2`.
- A bug fix adds the test that would have caught it, in the same PR.
- Platform-conditional tests (`#[cfg(windows)]`) exist only for genuinely platform-bound behavior (long paths, ACLs, CTRL_BREAK) and run in the matching CI lane.
- Coverage percentage is not a gate; the lifecycle matrix and the security list are.
