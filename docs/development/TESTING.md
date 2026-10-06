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
- Install script (`scripts/packaging/test-install-linux.sh`, runs in the bundle workflow): install → Exec rewritten → icons → idempotent upgrade → autostart on/off → broken-payload rejection → clean uninstall. Plus `shellcheck` on all packaging scripts and `desktop-file-validate` on the entry.
- The §23 proof of done — clean Win11 + Ubuntu/Arch VM installs — stays a documented manual step; `bundle.yml` exists so those VMs only ever install artifacts that already built, tested, and packaged green.

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
