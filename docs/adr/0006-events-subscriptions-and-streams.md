# ADR-0006 — Events, subscriptions, and streams

**Status:** Accepted · **Date:** 2026-10-05

## Context

Clients must observe server state, logs, and metrics in real time without polling, without a slow client stalling the daemon, and without unbounded memory. Reconnecting clients must reconstruct truth without guessing what they missed.

## Decision

### Streams

| Stream | Payload | Delivery | Replay model |
|---|---|---|---|
| `events` | state transitions (`server.state_changed`), job events, daemon events | immediate, per-subscriber queue | snapshot + bounded event ring (last 256 per server) |
| `logs` | parsed log lines, batched (~50 ms or 64 lines) | bounded channel; on overflow, coalesce with a `missed: N` marker | file-backed cursor (file identity + offset); rotation invalidates the cursor → client re-snapshots |
| `metrics` | 1 Hz per-server samples | latest-wins per subscriber | history via explicit range request, preallocated rings |

### Rules

- Every subscription is independent: **per-subscriber bounded queues**. Slow clients drop data with markers; they never backpressure the daemon.
- Every notification carries a monotonic `seq` (u64) assigned once, at ingest, per server per stream.
- Reconnect is always **snapshot + replay-from-cursor**. A cursor older than what replay can serve is answered with `cursorInvalid`, and the client re-snapshots — never an error-loop.
- The stdout reader task must never block on subscriber delivery. The pipe→parser→ring pipeline is decoupled by bounded channels with explicit overflow policies.
- The UI catches up on missed log stream sections through the file-backed log API, not by enlarging memory buffers.

## Consequences

- Latency targets: state change → notification p99 < 20 ms; log ingest ≥ 20k lines/s per server (see [PERFORMANCE-BUDGETS.md](../development/PERFORMANCE-BUDGETS.md)).
- The daemon's memory for streams is a function of *servers*, not *clients*.
- Metrics, implemented: the actor samples its live process at 1 Hz (`sample_process` in the platform seam — `/proc` on Linux, `GetProcessTimes`/`GetProcessMemoryInfo` on Windows), publishes through the hub into a 600-sample ring, and `metrics.range` serves the ring as history. Latest-wins is enforced at the subscriber: a full queue keeps the newest undelivered sample in a pending slot and flushes it when a slot opens — no `Missed` markers on this stream, because history comes from the ring, not from replay. The first sample after a process start carries `cpuPercent: null` (no counter base — an honest absence, never a guess across process generations).
