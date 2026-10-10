# ADR-0032 — The browser shell: what Chromium actually gives us, and the integration plan (P0.2)

**Status:** Accepted · **Date:** 2026-10-09

## Context

The founder's P0 directive: the shell must be **browser-grade, not a
React app pretending to be a browser**. Chromium's source is the
explicit reference; the order is inspect → identify reusable pieces →
determine what adapts → what stays ZIM-specific → integrate the
highest-value infrastructure first.

## What was inspected (Chromium)

Not screenshots — architecture, in four areas the shell's behavior
lives in:

1. **Tab strip machinery** — `chrome/browser/ui/views/tabs/`:
   `TabStripController`/`TabDragController` (drag state machine: press
   → detach threshold → move/insert index → drag-into-window →
   release/revert), `TabContextMenuController` (the menu as verbs over
   the tab model), pinned tabs and groups as **model states the strip
   renders**, never strip-owned.
2. **The omnibox** — `components/omnibox/`: `AutocompleteInput`
   classifies typed text into intents (URL / host:port / search /
   scheme'd) *before* any provider answers; the edit model (select-all
   on focus, Escape restores the pre-edit text, paste-and-go) is part
   of the controller, not the view.
3. **The command table** — `chrome/app/chrome_command_ids.h` +
   `chrome/browser/ui/browser_commands.cc`: one enumerated set of
   browser verbs (IDC_NEW_TAB, IDC_CLOSE_TAB, IDC_REOPEN_CLOSED_TAB,
   IDC_SELECT_TAB_n, IDC_SELECT_LAST_TAB, IDC_FOCUS_LOCATION,
   IDC_RELOAD, IDC_BACK, IDC_FORWARD, IDC_BOOKMARK_PAGE,
   IDC_SHOW_BOOKMARK_BAR, IDC_STOP…) with fixed chords and
   "works-inside-a-field" rules.
4. **Session/restore and crash isolation** —
   `components/sessions/` (tab restore as a service, not a strip
   feature) and the WebContents-per-tab model that makes a crashed
   renderer a per-tab event.

## The constraint, stated honestly

Chromium's UI layer (`views/`) is compiled C++ bound to aura/mus. It
cannot be embedded in ZIM's renderer, which is the Tauri webview
(WebView2 — itself Chromium — on Windows, WebKitGTK on Linux). Embedding
real Chromium UI would mean replacing Tauri with CEF/Electron and
rewriting every room (ZaminCore wiring, the protocol client, the
desktop plugins) around a different host. That is a platform rewrite,
not a shell correction — rejected, with the reason in writing rather
than a shrug.

## The decision

**Keep the Tauri webview (on Windows it already is Chromium rendering);
port Chromium's BEHAVIOR CONTRACTS as code + conformance tests, and
never fake what the host provides.** Concretely, three lanes:

1. **The command table is law** (landed this pass).
   `state/browserKeys.ts` implements the IDC table as one pure decision
   function; `browserKeys.test.ts` asserts each chord against the
   upstream behavior, including the field rules (Ctrl+L/F6/Alt+D work
   inside an input; Ctrl+D inside an input stays the editor's). The
   verbs execute against ZIM's destination model — Ctrl+T opens
   a new-tab page (§6), Ctrl+9 selects the last tab — browser chords,
   ZIM nouns.

2. **The omnibox rules are law, progressively.** The address bar's
   dialect classification already runs before resolution
   (`parseAddressInput`: internal URLs → host:port joins → free-text
   discovery, §58/§7) and resolution is registry-driven
   (P0 §13–§14: port + bind, no id-substitution). The edit model
   landed 2026-10-10 (PROVENANCE.md's omnibox section): Escape's
   two-stage restore (the first Esc reverts the display and KEEPS the
   focus with the permanent text selected; a bare Esc leaves),
   Alt-Enter's kNEW_FOREGROUND_TAB disposition (`land()` in
   `shell/omnibox.rs` — the classified request takes a fresh foreground
   tab that inherits the opener; a query waits on the tab itself and
   hello delivers it to the webview sync has not created yet),
   paste-and-go (Paste edits the field, Paste and go commits the
   clipboard text straight through the classifier), and the inline
   classification announced before the commit (the field's note names
   join address / search / ZIM page / no ZIM page). The suggestion
   popup landed the same day (`suggest()` + the popup machine): the
   exact join, the fleet's own names (from the frame's favicon-lane
   projection — no new daemon dial), the pages by prefix, the typed
   `console <name>` dialect, and the discovery search as the honest
   floor; the arrows walk the rows with the field showing the selected
   match, Enter/Alt-Enter commit through the same door, Escape reverts
   (popup first). Remaining: inline autocomplete and the history
   provider.

3. **The tab machinery follows the model/render split Chromium uses.**
   Drag (ADR-0018), groups (§49), pinned tabs (§52), tab tear-off and
   handoff (§50), restore (§90) already render from the tab model. The
   remaining Chromium conformance work is the drag state machine's
   edge cases (detach threshold, drag-into-window from another
   window) and the context menu's verb set — each lands as
   model-state + conformance test, never as strip-local state.

What stays ZIM-specific, permanently: destinations and
`zim://` URLs (§58), server tabs and their identity (§61),
Dutchmen, the feedback lane, extensions, publishing, Zamin Protocol.
The product is a browser redesigned around Minecraft infrastructure —
the browser's *discipline*, with the panel's own nouns.

## What was reused / adapted / is ours (the report table)

| Chromium | Disposition |
|---|---|
| IDC command table + chords + field rules | **Reused as behavior** (`browserKeys.ts` + conformance tests) |
| Omnibox input classification order | **Reused** (§58 dialects parse before any resolution) |
| Omnibox edit model (select-all, Escape restore, Alt-Enter, paste-and-go) | **Adapted** (2026-10-10: Escape's two stages, OpenURL's dispositions through `land()`, the paste verbs, the pre-commit announcement) |
| Omnibox popup (matches under the field, arrow selection) | **Adapted** (2026-10-10: `suggest()` from the classifier + the fleet projection, kMaxMatches 8, the arrow/click/commit machine; inline autocomplete and the history provider remain) |
| TabStrip model/render split | **Adapted** (tabs store renders the strip; drag/group/pin/restore ride the model) |
| TabDragController edge cases | **Adapting** (ADR-0018 covers reorder + insertion edge; thresholds/tear-in next) |
| Tab restore service | **Adapted** (§90 close memory + reopen in the tabs store) |
| Per-tab crash isolation | **Adapted** (§51/ADR-0016 `TabBoundary`, details inspectable since P0) |
| `views/` C++ UI layer, aura compositor | **Not embeddable** — constraint above |
| Network stack, sandbox, multiprocess | **Host-provided** (WebView2/WebKitGTK); never re-implemented |

## Consequences

- Every future shell change asserts against a behavior contract, so
  "feels like a browser" is testable instead of an opinion.
- The keyboard table stops drifting: chords land in one file, named
  after the IDC they mirror.
- The shell's remaining Chromium-conformance lanes (omnibox edit
  model, drag thresholds) are written down here as the queue — the
  next correction passes pull from it, and the founder's rule holds:
  if a change makes the panel prettier but not more browser-grade, it
  waits.
