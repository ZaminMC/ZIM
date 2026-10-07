# ADR-0016 — The tab machinery: identity, pinning, duplicate, groups, isolation

**Status:** Accepted · **Date:** 2026-10-08

## Context

ADR-0015 shipped the browser shell — the strip, the address bar, the destination router — and deliberately left the founder's tab machinery as reserved rooms: the §48 context menu carried only Reload / Close / Close others / Close to the right, and "Add tab to new group", "Move tab to new window", "Duplicate", and "Pin" were either absent or honest disabled entries. §90's required tab coverage (open, close, **duplicate**, **pin**, **group**, **reopen**, **crash isolation**) marked exactly what was missing. ADR-0015 also parked the question it could not answer yet: it derived a tab's stable identity from its destination — which makes Duplicate impossible, because two views of one server would share one key.

Two founder rules frame the whole slice. §61: *a tab, a server, a process, a directory, and an address are not interchangeable* — a tab is a UI destination. §65: *no duplicate backend* — duplicating a tab must never duplicate the server process or the daemon's ownership of it. And the scoping preamble still holds: the AI part stays ignored, its rooms kept.

## Decision

### A tab's identity is its own; the destination key is navigation's shorthand

Each `Tab` now carries a stable `id` (generated, unique, persisted with the store). The destination-derived key remains exactly what ADR-0015 made it — the **navigation identity**: navigating to a destination that already rests somewhere focuses that tab (preferring the active one when it rests there itself), never multiplying it. The strip, the active selection, close, duplicate, and pin operate on the `id`. This one split dissolves the contradiction the founder's two rules otherwise create:

- **Duplicate (§48, §65)** clones the *view*: a fresh id, the same destination history, a fresh reload token, landed unpinned and ungrouped right after the original, holding the focus. One server process now serves two isolated views — §51's "each tab has isolated UI state" makes this the intended shape, and §65 is honored because nothing behind the wire is copied.
- **Reopen (§90)** closes into a bounded most-recent-first memory (25, session-scoped, deliberately not persisted) and revives at the recorded strip position, clamped — pinned state restored, group membership deliberately not (a reopened tab is a fresh view). The auto-created fresh new tab that follows a last close is a birth, not a close: it never enters the memory.

### Pinning (§52) is a strip invariant, not a flag the UI happens to honor

Pinned tabs always sit at the head of the strip, each block in stable order; every mutation re-normalizes, so no code path can strand a pin mid-strip. The pinned tab renders compact (the favicon alone), hides its close button, and ignores the middle click — the accidental close — while the menu's Close stays explicit. Unpinning keeps the stable order (the tab becomes the head of the free block, exactly how a browser lets go of a pin). A pinned tab cannot join a group; pinning a grouped tab removes it from the group.

### Groups (§49) behave like browser groups — chips, never folders

A group is `{ label, color, collapsed }` in the store; membership is the tab's `groupId`. Joining a group moves the tab next to its group (the strip is presentation; identity never moves), so a group's members are contiguous and its chip has one place to live. The chip collapses on click (members hide; the count shows), renames on double-click (trimmed, 1–24 chars; an empty rename is refused), and colors from a six-color palette that cycles as groups are born. A group with no members dissolves — closing out, moving out, or pinning out its last member erases it. Adding to a new group takes the next palette color; moving between groups is a menu section, not a drag (drag reorder stays a follow-up when wanted).

### The context menu carries §48 with honest rooms

Live: New tab to the right (the new tab stays a singleton — a resting one moves over instead of cloning), Reload, Duplicate, Pin/Unpin, Add tab to new group…, Move to group ▸, Remove from group, Reopen closed tab, Close, Close others, Close tabs to the right. Reserved, named, disabled — each with a tooltip saying so: Mute (no audio machinery), Move tab to new window (real window management is its own slice, §50), Share tab with Dutchmen (the AI room, kept), Show tabs vertically.

### Isolation (§51) is a boundary with a recovery verb

The mounted tab content sits inside an error boundary. A view that throws renders "This tab crashed" — an honest page saying other tabs are unaffected — with **Reload this tab** as the recovery: §60's reload rebuilds the view and never touches the server process, so it is the crash recovery too. The content key is now `tabId:destinationKey:reloadToken`, which both isolates duplicate views' state from each other and remounts the boundary on recovery.

## Consequences

- Storage moves to version 2: v1 payloads (no ids, an `activeKey` destination string) migrate by assigning fresh ids and mapping the stored key to the first tab resting there. Torn payloads rehydrate as the default session; ids regenerate on collision; memberships without groups and groups without members cannot survive rehydration.
- The persistence boundary sharpens: `tabs`, `activeId`, `groups` persist; `recentlyClosed` and `discoveryQuery` stay session-scoped on purpose.
- The §90 tab list is now covered green at the store level (ids, duplicate, pin, group, reopen) and at the chrome level (compact pins, the menu, group chips, reserved rooms); crash isolation is covered at the shell level with a deliberately throwing workspace view.
- Still reserved rooms, named honestly: drag reorder, Move tab to new window (§50's real window machinery), Mute, the vertical strip, everything Dutchmen. None ship faked.
