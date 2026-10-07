# ZaminPanel Performance Budgets

*Budgets exist to catch architectural mistakes early, not for benchmark theater. Measured on reference hardware (mid-range 2023 laptop: 8 cores, NVMe, 16 GB) and in CI where the platform allows. Nightly perf job is a soft gate until Phase 3, hard gate after.*

## Daemon

| Metric | Budget |
|---|---|
| Log ingestion, per server | ≥ 20,000 lines/s sustained (parse → ring), < 15% of one core |
| Log ingestion burst | 50,000 lines/s for 10 s with zero unbounded memory growth; slow subscriber sees `missed: N`, daemon does not stall |
| IPC request/response round trip | p50 < 1 ms, p99 < 5 ms |
| State change → `events` notification delivery | p99 < 20 ms |
| Daemon RSS, idle | < 50 MB |
| Daemon RSS, 5 fake servers @ 1k lines/s | < 150 MB |
| Cold start → accepting connections | < 500 ms (p95 < 1 s; Windows AV makes p95 the honest number) |
| Metrics sampler cost | < 1% of one core per running server |
| Directory listing, 20k entries | p95 < 250 ms Linux, < 400 ms Windows |

## Log pipeline invariants

- The stdout reader never blocks on subscriber delivery (bounded channel + drop-with-marker).
- Ring buffers are preallocated and bounded by *servers*, not clients.
- A log cursor never re-reads a file from offset 0 to serve a tail.

## Panel

| Metric | Budget |
|---|---|
| Cold start → interactive | < 2 s Windows, < 3 s Linux (WebKitGTK) |
| Server switch (tab) | < 50 ms to committed state; animation runs on the compositor |
| Interaction → next paint | < 100 ms; no layout shift from state changes |
| Terminal input echo (round trip via daemon) | p99 < 50 ms |
| UI under load | no dropped frames with a background server streaming 1k lines/s |
| xterm.js scrollback | capped at 5k lines in view; full history belongs to the log viewer, not terminal memory |
| Stream delivery to webview | batched by the daemon's log pump — 10 ms flush tick with a 256-line cap (see Verifications); never one message per line under load |

## Verifications

Every budget above is a test in `crates/zamind/tests/perf.rs`, run by the
nightly `perf` workflow (`.github/workflows/perf.yml`); the normal
`cargo test --workspace` stays fast and hermetic. Measured on this repo's
development sandbox (2 cores, shared): IPC p50 ≈ 0.1 ms, cold start ≈ 100 ms,
sustained ingestion ≈ 42k lines/s (burst after a slow-subscriber stall
≈ 64k), echo p50 ≈ 10 ms, idle RSS 9 MiB, stalled-subscriber RSS 61 MiB.

Panel budgets are enforced by `apps/panel/perf/budgets.mjs` (`npm run
perf:budgets`, after `npm run build`): gzip sizes of the built chunks —
entry ≤ 90 KB, any single chunk ≤ 90 KB, total JS ≤ 170 KB, total CSS
≤ 12 KB. The cold-start payload was cut with code splitting: the entry
chunk went from 147 KB to 68 KB gzip by moving xterm into a console-tab
chunk (74 KB) and the three operator modals into their own chunks; the
console loads when the tab first renders, not at boot. Boot progress is
measurable in the running app through the `panel:boot-start` →
`panel:interactive` performance marks (`performance.measure("panel:cold-start")`),
recorded from browser smoke runs rather than guessed.

Metrics sampler cost is enforced where it is cheap to enforce:
`zamin-core`'s platform test asserts one `sample_process` call stays under
10 ms (measured in the low microseconds on Linux — two `/proc` reads — and
one syscall pair on Windows), which at 1 Hz puts the sampler far under the
1% of a core budget; the e2e suite asserts real samples flow (RSS measured,
CPU% from the second sample on) and that cadence, ring, and range stay
honest. Server-switch and interaction budgets are covered structurally:
the workspace's live header chips subscribe to the metrics store alone, so
a 1 Hz flood re-renders two chips and never the console — asserted by a
render-count test in `ServerView.test.tsx`.

Negotiations and hardware notes, in writing per the rules:

- **Terminal echo ↔ flush tick.** Echo latency is quantized by the pump's
  flush tick, so the original 50 ms tick made the p99 < 50 ms budget
  unsatisfiable by design. The tick is 10 ms, which keeps echo p99 ≈ 10–30 ms
  on the sandbox while flood batches stay at the 256-line cap (avg ≈ 239
  lines — the "never one message per line" invariant holds).
- **Slow-subscriber memory ↔ batch cap.** The subscriber queue bounds
  notification COUNT (1024); without a per-notification size bound, backlog
  memory scaled with ingestion rate (measured 182 MiB under an unbounded
  flood before the cap). The pump's 256-line cap makes each frame's size
  rate-independent; measured stalled-subscriber RSS dropped to ~61 MiB.
- **Burst rate in CI.** The nightly gates on the sustained 20k lines/s plus
  the invariants (no stall, missed marker, bounded memory, batch shape);
  the 50,000 lines/s burst figure is a reference-hardware number, recorded
  from every nightly run's uploaded log rather than gated on shared runners.
- **Memory assertions are Linux-only** (`/proc`); the timing and invariant
  assertions run everywhere the suite runs.

## Rules

1. Budgets are tested, not asserted by belief: the perf suite spawns the real pipeline and measures.
2. A PR that touches a hot path (log pipeline, IPC, fs ops, stream delivery) states its measured impact in the PR description.
3. Regression > budget → fix before merge, or negotiate the budget in writing (edit this file in the same PR) with a reason.
4. Guessing is not measuring: any "should be fast enough" claim about a hot path gets a benchmark or gets dropped.
