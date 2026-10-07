# ADR-0015 — The browser shell: tabs, destinations, and the address bar

**Status:** Accepted · **Date:** 2026-10-08

## Context

The founder vision (`docs/founder-vision.md`) names the product identity precisely: **ZaminPanel is a browser for Minecraft servers**, and §84 makes the browser shell the first implementation target — tabs, address bar, new tab, discovery, server tab, status, console, end to end. The current shell is a sidebar-first control center, which the vision explicitly forbids ("do not build a sidebar-first admin panel"). The vision's own preamble also gives a scoping rule this ADR follows: **the AI part (Dutchmen) is explicitly ignored, but a room is kept for it.** Every Dutchmen-shaped surface — the new-tab transition (§8), chat tabs, "Ask Dutchmen" actions, the tool registry — stays out; the rooms it would occupy are documented here and in code, not faked (§82: no fake functionality).

The panel already owns the hard parts the shell sits on: the wire (events, subscriptions, jobs), the servers store, the per-server workspace (console, logs, files, players, plugins, schedules, backups), the crash card, and the modals. What is missing is the frame: tabs as first-class objects, an address bar that speaks the product's dialects, and navigation history. The vision's identity discipline (§61) is the load-bearing rule: a tab, a server, a process, a directory, and an address are not interchangeable.

## Decision

### Destinations are a closed type; tab identity derives from them

`state/destinations.ts` (committed before this ADR's shell) is the typed core: a `Destination` is `servers | new | server(serverId)` — never a string, never a web page (§58). A tab's stable key derives from its destination on purpose: **one tab per server, one fleet page, one new-tab page.** Opening an already-open destination focuses its tab; navigation never duplicates identity. Every destination names itself with a canonical internal URL (`zaminpanel://servers/`, `zaminpanel://new`, `zaminpanel://server/<id>`).

### The shell is the browser chrome; the content is a destination router

The sidebar retires. The frame becomes: a **tab strip** (tabs, close buttons, new-tab button), a **tool bar** (back, forward, reload, the address bar, the system cluster), and the content area rendering exactly one destination:

- `zaminpanel://servers/` — the fleet page: the registry as cards, honest counts, discovery. The former Dashboard, renamed and re-scoped.
- `zaminpanel://new` — the new-tab page: a centered discovery input and the active servers with opening actions (§22's deterministic form). Free text here is a **discovery query over the registry** — substring over display name and id, case-insensitive. It is not a web search box and, for now, not a chat box either: the Dutchmen transition is a reserved room.
- `zaminpanel://server/<id>` — the server workspace, unchanged underneath the new frame.
- `zaminpanel://settings/` — an honest internal page: connections, daemon identity, and the reserved-rooms list. Unavailable things say so.

### The address bar speaks three dialects, parsed before any store is consulted

1. **Internal URLs** (`zaminpanel://…`) navigate to the named internal page; an unknown internal page is answered honestly ("no such page"), never silently as a search.
2. **Join addresses** (`localhost:25565`, `0:25565`, `box.example.com:25565`) resolve against the registry — **the port is the match; a typed host only agrees or disagrees** with this window's own reach (localhost for the local daemon, the box's host for a remote profile). Bind-all and loopback spellings (`0`, `0.0.0.0`, `127.0.0.1`, `::`) normalize to the same local server. An unresolvable join address is refused honestly at the bar — it does not navigate, and it does not pretend.
3. **Free text** is a discovery query, executed on the new-tab page.

A fourth dialect — `dutchmen:(chat-id)` — is a **documented reservation**: the parser will grow it when the AI part lands, and nothing ships that pretends it exists today.

### History and reload mean what they mean in a browser

Back/forward operate on **per-tab destination history** (§59): only actual destination changes become history entries; UI clicks do not. Reload (§60) rebuilds the active tab's view: state refreshes, event subscriptions reconnect, unsaved nothing is pretended preserved — but the server process is never touched. Reload must never mean "restart Minecraft."

### Tab state moves out of the UI store into a tabs store

`openTabs`/`activeTab` strings in the UI store were server ids wearing a trench coat. They move to `state/tabs.ts`: ordered tabs of `{ history, historyIndex, reloadToken }`, persisted (the session restores), with pinning presented later if at all. Per-server verb-in-flight and action-error state stay in the UI store — they are operation feedback, not navigation.

## Reserved rooms (documented, not faked)

Dutchmen new-tab transition and chat tabs (§8–§21), tab groups (§48–§49), move-tab-to-window (§50), pin/mute presentation (§52–§53), vertical tabs (§54), the bookmarks bar (§55), extensions (§56), publishing. Each lands only with its real machinery behind it.

## Consequences

The shell tests rewrite around browser flows (open, navigate, back, close, address dialects, singleton identity). The sidebar's keyboard surface (Ctrl+K palette) stays, joined by browser keys (Ctrl+T/W/L, Alt+arrows, Ctrl+Tab). The DESIGN test (§91) now has a concrete arbiter: if a proposed change makes the shell feel like a control panel again, the change is wrong.
