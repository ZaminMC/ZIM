# ADR-0023 — The bookmarks bar and the crash card's verbs (§55, §62)

**Status:** Accepted · **Date:** 2026-10-08

## Context

Two founder surfaces were still rooms without furniture:

- **§55** — the bookmark bar: a browser-style bar that can hold servers,
  server tabs, Dutchmen conversations, and internal pages, where
  bookmarks preserve destination identity.
- **§62** — crash handling: the tab stays open, the status turns red,
  and the operator reads "Server stopped unexpectedly." with a Reason
  and the verbs [Restart], [View logs], [Ask Dutchmen] — where Dutchmen
  inspects the crash honestly or says nothing when the evidence is thin.
  The crash card existed (ADR-0005's classification + ADR-0021's backup
  doorway) but its verbs were the panel's, not the founder's.

The scoping preamble holds: the AI rooms stay reserved, untouched —
"Ask Dutchmen" arrives with the agent runtime and gets a labeled seat,
not a fake handler.

## Decision

### Bookmarks hold typed destinations, and identity is inherited

`state/bookmarks.ts` persists `{ id, label, destination }` rows where
`destination` is the tabs model's typed union — never a string. Opening
a bookmark calls the tabs store's `navigate`, so §61's singleton focus
(a bookmark to a resting destination focuses that tab) and the §6
"one tab per server" discipline are inherited, not reimplemented. One
destination holds one bookmark: a second add is a no-op, and the address
bar's star reflects membership instead of stacking rows. A new
`openInNewTab` action covers the explicit new-tab gestures (Ctrl/Cmd+
click, middle-click, the context menu) — an explicit request, so it does
not reuse the resting new-tab page the way a plain new tab does.

The bar is panel-local and shared across windows (Chrome's model: the
bar is global, the tabs are not). Torn persisted rows are dropped at
load, never rendered. The Dutchmen conversation kind needs no migration
when the agent runtime lands — the bar renders whatever destinations
exist, and the union grows with the model.

The star lives in the address bar (the browser convention): present for
every destination a bookmark could hold, absent on the new-tab page —
a room, not a destination worth keeping. The bar toggles from the ⋮
menu and Ctrl+Shift+B, and hides itself when empty or hidden — an empty
bar renders nothing.

### The crash card speaks the founder's sentence

The card now leads with "Server stopped unexpectedly.", carries the
facts the daemon classified (phase, exit code, error code), and a
Reason line that is honest twice: it prefers the structured error
message, falls back to the evidence excerpt's first line, and when
nothing was captured it says exactly that instead of inventing a cause
(§82 — the panel never guesses). The full evidence excerpt stays
available beneath the reason; remediation steps render as steps.

The verbs are the founder's:

- **[Restart]** dispatches the real lifecycle verb through the SAME
  pending/actionError fields the header uses — one in-flight verb shows
  once, one failure surfaces once — and the card resolves itself when
  the registry's next transition proves it stale. The button does not
  pretend the server came back; it dispatches and waits.
- **[View logs]** jumps the workspace to the log viewer.
- **[Ask Dutchmen]** is present, disabled, and labeled as the reserved
  room it is ("Reserved — Dutchmen inspects crashes once the agent
  runtime arrives"). A missing seat would be a silent lie of omission;
  a fake one would be worse.
- Acknowledge (the ×), and the ADR-0021 backup doorway remain — the
  quiet exits.

### The stale-card rule closes its last gap

`applyState` resolved a card on the next transition, but details
fetched after a `registered` event (which can carry a live state)
bypassed it — a stale card could outlive its own crash. `upsert` now
applies the same rule: any state other than `crashed` proves the card
stale. The card lives exactly as long as the registry says "crashed",
no longer.

## Consequences

- The bookmarks store, the bar, the star, and the crash card's verbs
  are all panel-local — no protocol or daemon changes, no wire growth.
- `tabs.openInNewTab` is the first navigation verb that bypasses the
  resting new-tab page; its docstring says why, so the §6 discipline
  stays readable.
- The crash card test suite now pins the founder's sentence, the no-
  invented-cause rule, the reserved Dutchmen seat, and the upsert rule;
  ServerView's tests re-keyed to the new labels.
- The panel CSS budget raises 14 → 15 KB (bookmarks bar stylesheet +
  the crash reason row) — written down in PERFORMANCE-BUDGETS.md per
  the rules, with the boot path untouched.
