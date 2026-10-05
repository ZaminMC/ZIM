# ZaminPanel Testing Conventions

*Testing follows the architecture: the lifecycle matrix, the protocol, and the filesystem safety model are the load-bearing surfaces, so they are the best-tested.*

## Layers

| Layer | Where | What |
|---|---|---|
| Unit | `#[cfg(test)]` beside the code | state machine transitions (table-driven), config parsing/migration, log parser, port logic, path containment |
| Integration | `crates/*/tests/` | real filesystem, real processes (fake-mc-server), IPC client ⇄ daemon |
| Protocol conformance | `crates/zamin-protocol/tests/fixtures/` | recorded exchanges asserted across versions; breaking changes must update fixtures intentionally |
| Performance | `#[ignore]`d tests run by the nightly CI job | budgets from [PERFORMANCE-BUDGETS.md](PERFORMANCE-BUDGETS.md) |
| UI | `apps/panel` — vitest component tests; visual states later | components render states (default/hover/focus/disabled/loading/error/selected); critical flows once a test harness exists |

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

Lifecycle tests spawn it through the real supervisor and real IPC. No test may sleep-and-hope: state changes are awaited via events.

## Lifecycle matrix (required coverage)

start · stop · restart · crash during startup · crash while running · stop timeout → ladder escalation · hang on stop · duplicate start · start during stopping · adoption after daemon restart · foreign PID in registry (never adopt, never kill) · kill tree (child of child) · each preflight failure · port conflict · daemon restart while running · client disconnect/reconnect with cursor replay.

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
