# ADR-0020 — The console slice: filters, copy control, and the dedicated console tab (§26–§30)

**Status:** Accepted · **Date:** 2026-10-08

## Context

The founder's console (§25) is the server page's default view, and §26–§30
spell out its obligations: the §26 command input, the §27 open-in-new-tab
icon leading to a dedicated console-only tab "optimized for large console
output", the §28 copy control (one button, mode changed by right-click —
red for All errors, yellow for All warnings, default for All), the §29
filter row (Show all | Info | Warnings | Errors) that "should not destroy
the underlying log stream", and §30's performance rules (bounded buffers,
event cursors, replay, "the console UI must not become the bottleneck").

The console that existed was an **xterm.js terminal**. A terminal grid is
an excellent stdin surface but the wrong shape for the founder's console:
§29's filters cannot apply to already-written ANSI cells without
destroying and rewriting them, §28's mode-tinted copy control has nothing
structured to copy from, and §30's economy was inverted — the heaviest
dependency in the panel was lazy-loaded to render a view capped at 5,000
scrollback lines while the full history sat in a file the terminal would
never page. The structured machinery the founder's rules want already
existed in the repo: LogViewer's file-backed pages, byte-offset cursor,
live stream join, and level chips (ADR-0006's file-is-the-truth discipline).

The scoping preamble still holds: the AI part stays ignored, its rooms
kept. Nothing here reaches Dutchmen.

## Decision

### One feed engine, two views — the machinery extracted, never forked

The log feed (history pages + live join + seam + paging + follow) moves
verbatim from LogViewer into `state/logFeed.ts`. Both structured views
render from it: the Logs workspace tab and the console. One seam
discipline (`stripLiveOverlap`), one `LOG_CURSOR_INVALID` recovery, one
bounded buffer — console behavior can never drift from log behavior.

### The console is the structured view

The xterm terminal is **removed** — component, its pure helpers, and the
four `@xterm/*` dependencies. The console (embedded under the server page
and dedicated as its own tab) is LogViewer's engine wearing the console's
obligations:

- **§29 filters** — Show all | Info | Warnings | Errors, plus Debug (the
  level exists in the model; the row answers it rather than pretending
  the stream has four levels). Filtering is a render-time view over the
  intact buffer; switching back proves nothing was destroyed.
- **§28 copy control** — one button in the console bar. Left-click
  performs the selected mode; right-click opens a small contextual menu
  (a modal would be ugly, the founder says so). The tint answers the
  mode: red errors, yellow warnings, default All. The copy is honest
  about its reach — the **loaded view** (the loader pages older lines in;
  the file holds everything), the tooltip says so, and an empty result
  reports "no error lines in the loaded view" instead of pretending.
  Lines copy as `[level] [thread] message` plain text.
- **§26 composer** — the command input at the bottom, over the ordinary
  `server.stdin` path. Disabled while the server is not running, with
  the honest note; a stdin rejection surfaces instead of vanishing.
- **§30 performance** — inherited from the engine: paged history by byte
  offset, live lines buffered while scrolled away, cheap rows with
  `content-visibility`, and the file as the unbounded memory.

### §27: the dedicated console tab is a typed destination

`{ kind: "console", serverId }` joins the closed destination set — URL
`zim://console/<id>`, tab key `console:<id>` (§61: navigating
again focuses, never duplicates). It rests at its internal URL, not the
server's join address — it is a ZIM page. The tab strip titles it
"<name> console" and wears the terminal icon. Every tab operator works on
it unchanged (duplicate, pin, group, drag, §50 move-to-window, reload)
because it *is* a tab, per §27's "it should still behave as a normal
ZIM tab". The workspace console's bar carries the §27 icon; the
dedicated tab does not carry it again.

### Budgets move deliberately, or they do not move

Removing xterm deletes the panel's heaviest dependency; the console chunk
collapses to the shared feed engine. Entry and total budgets stay where
ADR-0019 pinned them — a shrinking total is recorded, never banked as
headroom for the next regression. PERFORMANCE-BUDGETS.md carries the note.

## Consequences

- The founder's §25–§30 are now real: input, dedicated tab, copy modes,
  filters, and the performance posture — with no second log system to
  keep honest.
- `consoleText.ts` (ANSI formatting, keystroke buffering) is gone with
  its terminal; stdin travels from a plain form field.
- The dev-bridge browser loop needs no canvas; the console renders and
  tests in jsdom like every other view.
- Testing: console tests cover the non-destructive filter round trip,
  per-mode copy with the contextual menu and the empty-mode honesty, the
  composer's send/disabled/rejection paths, and the §27 icon's presence
  rules; destinations tests pin the new key, URL, parse, and resting
  address; LogViewer's suite runs unchanged against the extracted engine
  (the shared seam included).
