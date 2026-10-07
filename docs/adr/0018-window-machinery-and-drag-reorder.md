# ADR-0018 — The window machinery: per-window strips, the §50 handoff, drag reorder

**Status:** Accepted · **Date:** 2026-10-08

## Context

ADR-0016 landed the tab machinery and left two named rooms: drag reorder was
recorded as a follow-up ("a menu section, not a drag — drag reorder stays a
follow-up when wanted"), and **Move tab to new window** (§50) was a disabled
entry waiting for "its real machinery". §90's required tab coverage names
**move window** outright. The founder's words for the move are strict about
what it must not do — restart the server, duplicate the server process,
duplicate daemon ownership, corrupt server state — because *the underlying
server remains owned by ZaminCore; the UI window is only a client*.

The feature forces a question ADR-0015/0016 never had to answer: where does a
second window's strip live? Until now the tab store persisted to one shared
localStorage key. Two windows would clobber each other on every write — the
move feature would have shipped a data-loss bug as its first impression. The
scoping preamble still holds: the AI part stays ignored, its rooms kept.

## Decision

### Every browsing context owns its strip; the identity is minted once per boot

The panel is one SPA that can live in several windows at once, so the tab
store's persistence is keyed **per window**: `zamin-panel.tabs@<windowId>`.
The identity is minted once per boot into `sessionStorage` — it survives a
reload (the strip must survive it too) and dies with the context. The hard
part is telling a *reload* from a *birth that was handed a copy of the
opener's sessionStorage* (browsers copy it whenever a context opens with an
opener — exactly what the move does). The discriminator is Navigation Timing:
`reload`/`back_forward` reuse the stored id; a declared `navigate` birth mints
fresh; a handoff birth (`#handoff=` in the URL) always mints fresh. When the
platform stays silent about navigation (old webviews, test DOMs), an existing
id is **reused** — losing a strip on reload is the one unrecoverable mistake
this module refuses to risk. A denied sessionStorage mints in-memory: the
window works; its strip just does not outlive it.

A registry (`zamin-panel.windows`) records each window's last-active time.
Boot prunes windows idle past two weeks and deletes their strips; a torn
registry is rebuilt and prunes nothing. Boot also forces one persist write so
a window that boots and reloads without touching anything still finds its own
strip. The pre-ADR-0018 shared key is **adopted exactly once** — the first
fresh window inherits it and retires it, so the second window is born fresh
rather than stealing the same tabs.

### The §50 move is a handoff, not a close

`moveToNewWindow` writes a handoff slot keyed by a fresh handoff id
(`zamin-panel.tab-handoff:<id>` — keyed, because two moves can be in flight
and a slot must never be claimed by the wrong child), then removes the tab
from the origin strip. **A move is not a close**: the tab enters no
recently-closed memory, because it still exists — in the window that is about
to open. The focus moves exactly like a close when the active tab leaves and
stays put otherwise. The origin then opens the new context with
`#handoff=<id>` in its URL.

The child claims the slot at boot, before first render (a boot module that is
main.tsx's first import, so the identity exists before the store is created
and a moved tab arrives with no flash of the new-tab page): the moved tab
becomes that window's whole strip — fresh tab id, pinned state preserved,
**group membership deliberately dropped** (groups are strip-local; membership
cannot travel), the handoff hash consumed via `history.replaceState` so a
reload boots as a plain reload. A torn, foreign, or stale slot (60 s TTL — a
handoff is claimed by a window opening *now*) is refused, never half-claimed.

The opener checks the handle: `window.open` returns null when a popup blocker
refuses the window, and a blocked popup must not swallow a tab. The opener
removes the unclaimed slot and calls `restoreMoved` — the tab comes home to
its recorded index, rejoining its group if the group survived, with the focus
restored exactly when the move had taken it.

§50's prohibitions hold by construction: a tab is a view; the move writes
localStorage and re-renders two strips. Nothing behind the wire is touched —
no restart, no duplicate process, no duplicate ownership. The child window
establishes its own client connections, precisely as any second browser
window of the panel already would; the server stays daemon-owned. In the
browser delivery the "new window" is a new browsing context of the same SPA
(the founder's window model maps onto OS windows in the desktop shell; that
mapping is future machinery and this ADR does not fake it).

### Drag reorder (§90) honors the strip's two invariants

`reorder(id, insertion)` takes the **visual** insertion slot in the current
strip and re-anchors after removal. The pinned head clamps both ways: a
pinned tab drags within the pinned block (0..pinnedCount of the others), a
free tab never lands before the block. Group membership follows the pointer's
intent: dropping within reach of the mover's own group (beside one of its
members) keeps membership — the strip then re-normalizes so the group's
members stay contiguous — and dragging out of the group's reach **leaves the
group** (browser-honest; the group survives if it still has members). A drop
never *gains* membership: joining a group stays the §49 menu's explicit verb,
because a drag that silently groups tabs would make the chip a trap.

The strip's chrome uses HTML5 drag-and-drop: every tab is `draggable` (pinned
ones included), the pointer's side of the hovered tab decides
before/after — one `sideOf` helper serves both the visible insertion edge
(an accent bar via `.dropBefore`/`.dropAfter`) and the drop math, so the
indicator and the outcome can never disagree. The dragged id rides a ref
(dragover cannot read the payload), `dataTransfer` carries the id for
Firefox's payload-less-drag refusal, and `dragend` clears every trace.
The drag never steals the focus.

## Consequences

- Storage moves to version 3. The payload shape is unchanged from v2 (v3 only
  moved the storage key per window); v1→v2→v3 migration still runs through the
  same sanitize path. Torn payloads rehydrate as the default session, per key.
- New boot housekeeping runs per context: registry touch, prune, handoff
  sweep, one persist write. All of it is bounded, defensive against torn
  JSON, and safe to run twice.
- The `#handoff` hash is consumed state, not an address: reloads of a moved
  child boot normally, and a stale hash with no slot is a quiet no-op.
- §90's tab list gains its **move window** row: covered at the store level
  (handoff write, claim, foreign/stale/torn refusal, restore, last-tab
  refusal, no close memory), at the chrome level (the live verb, the honest
  last-tab disable, the blocked-popup restore, drag through the strip), and
  the identity/storage layer (per-window keys, one-time legacy adoption,
  prune, sweep).
- Panel bundle budgets: the slice costs ~2 KB of the remaining headroom
  (170.2/170 KB). The next feature slice should expect to revisit the budget
  line in the same breath as its first commit.
- Still reserved, named, disabled: Mute (§53), Share tab with Dutchmen
  (the AI room), Show tabs vertically (§54). Nothing pretends.
