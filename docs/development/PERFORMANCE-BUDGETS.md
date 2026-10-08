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
| Console view bounds | paged history by byte offset + the live buffer; full history belongs to the file (ADR-0006), the view renders the loaded window with `content-visibility` rows (ADR-0020) |
| Stream delivery to webview | batched by the daemon's log pump — 10 ms flush tick with a 256-line cap (see Verifications); never one message per line under load |
| Long-list first paint (files table, player roster) | ≤ 120 rows committed with the listing (`ui/deferred.ts` window); a 2,000-entry directory costs the same interaction latency as a 100-item one; the remainder lands over idle frames and never blocks interaction |

## Verifications

Every budget above is a test in `crates/zamind/tests/perf.rs`, run by the
nightly `perf` workflow (`.github/workflows/perf.yml`); the normal
`cargo test --workspace` stays fast and hermetic. Measured on this repo's
development sandbox (2 cores, shared): IPC p50 ≈ 0.1 ms, cold start ≈ 100 ms,
sustained ingestion ≈ 42k lines/s (burst after a slow-subscriber stall
≈ 64k), echo p50 ≈ 10 ms, idle RSS 9 MiB, stalled-subscriber RSS 61 MiB.

Panel budgets are enforced by `apps/panel/perf/budgets.mjs` (`npm run
perf:budgets`, after `npm run build`): gzip sizes of the built chunks —
entry ≤ 96 KB, any single chunk ≤ 96 KB, total JS ≤ 184 KB, total CSS
≤ 21 KB with the entry stylesheet's own line held at ≤ 11 KB. The cold-start payload was cut with code splitting: the entry
chunk went from 147 KB to 68 KB gzip by splitting the console and the
three operator modals into their own chunks; the console loads when the
tab first renders, not at boot. The ADR-0019 raise (170 → 184 KB JS,
12 → 14 KB CSS) bought the three configuration surfaces — Startup,
network, Settings — as new lazy chunks (~6.5 KB JS gzip combined, ~1 KB
CSS); the entry chunk was untouched by that slice (cold start →
interactive stays the real budget), so the growth is features arriving,
not the boot path regrowing. ADR-0020 then removed xterm — the panel's
heaviest dependency — by making the console the structured view; the
console chunk collapsed to the shared feed engine and the budget lines
stayed where ADR-0019 pinned them (a shrinking total is recorded, never
banked as headroom for the next regression). The ADR-0023 raise (14 → 15 KB CSS) bought the
bookmarks bar (its own chrome stylesheet, shared across windows) and
the scoreboard editor's two-pane layout (riding the files chunk); the
entry CSS grew only by the crash card's reason row, and the boot path
itself is untouched. The ADR-0024 raise (90 → 96 KB entry/any-chunk JS)
bought the update lane: the store, the chrome notice, and the Settings
updates rows ride the entry because the boot check is the lane's first
duty; the desktop plugin APIs stay lazy chunks, so the growth is the
panel's own decisions, not the OS calls. The ADR-0025 raise (15 → 16 KB CSS)
bought ui/ErrorNote's single stylesheet — one shared error body (sentence,
remediation, [View details] disclosure) rendering at fourteen call sites,
instead of bespoke error CSS per view. The ADR-0026 raise (16 → 18 KB CSS)
bought the evidence pages — jobs (§73), audit (§72), about (§58) — whose
stylesheets ride their own lazy chunks (a page's CSS loads when the page
opens, never at boot), plus the vertical rail's rules in the strip's chrome
stylesheet (§54). The entry stylesheet's growth in that slice was the mute
indicator alone; a new explicit entry-CSS line (≤ 11 KB) now holds the cold
start's CSS so a chrome-only regression cannot hide inside the total. The
ADR-0028 raise (18 → 19 KB CSS) bought the feedback lane: the page's own
stylesheet and the settings account row, both lazy-chunk CSS (the settings
page itself moved to the same lazy lane as the other internal pages in the
same slice, which PAID BACK ~3 KB of entry JS — the boot path ended this
negotiation lighter than it entered it). The ADR-0031 raise (19 → 20 KB
CSS) bought the extensions room's own stylesheet — a lazy-chunk file
(~0.76 KB gzip) for the §56/§57 inventory page: rows, permission chips
split by family, and the problems panel; the entry stylesheet was
untouched by the slice, and the growth loads when the page opens, never
at boot. The downloads room (§58's reserved URL, live now that a
versioned channel exists — ADR-0029) took the line to 21 KB in the same
batch: its page-specific rules ride their own lazy chunk, and the
internal pages' chrome (page, heading, notes, evidence rows) was
factored into one shared lazy stylesheet, `internalPage.module.css`, so
the next room pays only its own rules. The entry stylesheet's line did
not move in either raise. Boot progress is
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

Long-list rendering is enforced by `apps/panel/src/ui/deferred.test.tsx`:
a 2,000-entry collection commits its 120-row window synchronously (the
frame the listing pays for), grows 240 rows per idle frame until done,
and resets for free when a fresh listing arrives — the reset happens
during render, so no frame ever carries the full old window over the new
rows. "Show all" is the one explicit full commit, on demand. The files
table (daemon-capped at 2,000 entries) and the player roster use the
window; the paint cost tracks the window, not the directory size.

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
