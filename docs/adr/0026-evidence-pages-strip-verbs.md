# ADR-0026 — The strip's last two verbs and the evidence pages (§53, §54, §58, §72, §73)

**Status:** Accepted · **Date:** 2026-10-08

## Context

Five founder surfaces were still rooms:

- **§53 Mute** — a tab's audio posture, shown on the tab. The strip's menu
  carried the honest reserved room ("Planned — audio controls land with
  their real machinery"), and no machinery existed to land.
- **§54 Vertical tabs** — the same tab objects rendered as a rail, the
  Discord/browser vertical navigation shape rather than a generic
  sidebar. Also a reserved room.
- **§58's page list** — `zim://jobs/`, `zim://audit/`, and
  `zim://about/` were named as future internal URLs while the
  typed destination model already existed to carry them.
- **§72 Audit** — the daemon has appended every mutating command and
  handshake to a JSONL trail since ADR-0011, and the CLI reads it
  (`audit.list`); the panel had no seat at that table.
- **§73 Jobs** — the daemon's job runner publishes `job.started` /
  `job.progress` / `job.completed` and answers `jobs.list` /
  `jobs.cancel`; Backups shows one job's chip, but no surface showed ALL
  the long-running work.

The scoping preamble holds: the AI rooms stay reserved, untouched.

## Decision

### Mute is tab state, not a button (§53)

`Tab.muted?: boolean` joins the persisted tab model (v3 → v4; the
sanitizer defaults an absent field, a torn one is refused with the rest
of the payload). The posture is data: every audio-producing surface — an
extension's UI when §56 lands, any future media view — must consult it
before it emits a sound, and the strip shows the state the moment it is
set (a muted mark; the indicator IS the state, not a control). The menu
verb flips it ("Mute tab" / "Unmute tab"). The posture travels with the
view exactly like pinned: duplicate keeps it, the close memory keeps it
and reopen restores it, and the §50 handoff carries it — optionally on
the wire, so a slot written by an older build still claims cleanly.

### Vertical is presentation, nothing else (§54)

`verticalStrip` is a per-window pref in the tabs store (persisted with
the strip, defaulted horizontal). The strip renders the same tab
objects as a left rail: full-width rows, the pinned head compact at the
top, groups as stacked chips, and drag reordering that follows the rail's
axis — `sideOf` splits above/below from the pointer's Y when vertical,
and the insertion edge draws above/below (`dropAbove`/`dropBelow`). The
menu item toggles it and says so ("Show tabs vertically" / "Use
horizontal strip"), and the strip declares its `aria-orientation`. No
mutation, no identity rule, and no test seam changed shape — the
presentation is exactly as deep as presentation.

### The evidence pages ride the typed destination model (§58, §72, §73)

Three destinations join the closed union — `jobs`, `audit`, `about` —
each a window singleton with its `zim://` URL, its address-bar
route, its resting address, and its label. They are lazy chunks (a
page's code and CSS load when the page first opens; the entry keeps the
shell + fleet page), and the palette offers all three as commands, so
the keyboard speaks the same destinations the address bar does.

- **JobsPage (§73)** seeds from `jobs.list` (the daemon's authoritative,
  reconnection-proof record — the newest 50 finished jobs survive any
  UI restart) and then rides the live `job.*` events already flowing
  into the jobs store. A row is the job's facts: kind, id, server,
  state chip (queued / running / succeeded / failed / cancelled), the
  progress meter when the daemon reports a total (a progress block
  without one says what it has and never invents a percent), started
  and ended times, the job's own typed error through the §81 note, and
  a Cancel verb only while the state can still change — the flip itself
  stays the daemon's, at the job's next checkpoint. The empty state is
  honest, and a refused seed is a typed alert instead of a fake
  "nothing is running" (§82).
- **AuditPage (§72)** reads `audit.list` newest-first, paged (100 to a
  page, "Load older entries" walking the offset backward). A row is the
  line the daemon appended: when, the method, the server, the outcome
  chip (ok, or the error code the daemon answered with), and the
  protocol client that asked. Malformed lines are counted in plain
  words instead of dropped, because the file is the schema and the page
  does not pretend otherwise. The footer states the read rule: reads
  are not audited, so the page never appears in its own trail.
- **AboutPage (§58)** states what the host actually answered: the
  installed version (or "the host has not answered yet" in a dev
  session), the development channel and its signing discipline, the
  connected daemon's identity, and the reserved rooms — Dutchmen,
  extensions, the conversation address dialect — named with their
  founder sections rather than faked (§82). The restart rule is
  written where the operator can read it: an automatic install never
  restarts the session on its own (ADR-0024).

## Consequences

- The tabs store persists at version 4; v1–v3 payloads migrate with
  muted defaulting off and the pref defaulting horizontal.
- `TabStrip` grows no new state of its own: mute and vertical are the
  store's, the drag axis is a prop of one helper, and the menu's two
  reserved rooms become verbs. The strip's honest-rooms test now pins
  exactly one reserved entry: "Share tab with Dutchmen".
- The panel speaks `audit.list` for the first time (typed parameters,
  results, and a `listAudit` action); the wire itself never grew — the
  daemon answered these methods since ADR-0011/ADR-0014.
- The palette's `buildCommands` gained an optional `pages` argument;
  existing call sites and tests keep their shape.
- The CSS budget raises 16 → 18 KB (three page stylesheets on lazy
  chunks + the rail's rules), written down in PERFORMANCE-BUDGETS.md
  with a new explicit entry-CSS line (≤ 11 KB) so the boot path's CSS
  cannot hide inside the total. The entry chunk itself is untouched:
  the three pages are lazy, and the cold start ships the shell + fleet
  page exactly as before.
