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
| Stream delivery to webview | batched ~50 ms via the bridge channel; never one message per line |

## Rules

1. Budgets are tested, not asserted by belief: the perf suite spawns the real pipeline and measures.
2. A PR that touches a hot path (log pipeline, IPC, fs ops, stream delivery) states its measured impact in the PR description.
3. Regression > budget → fix before merge, or negotiate the budget in writing (edit this file in the same PR) with a reason.
4. Guessing is not measuring: any "should be fast enough" claim about a hot path gets a benchmark or gets dropped.
