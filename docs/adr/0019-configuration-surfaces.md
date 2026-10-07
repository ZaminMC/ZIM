# ADR-0019 — The configuration surfaces: Startup, Network, Settings (§37–39)

**Status:** Accepted · **Date:** 2026-10-08

## Context

The server page's sidebar (§23) names six sections; sessions 1–12 landed
Console, Files, and Schedules — but **Network (§37), Startup (§38), and
Settings (§39) did not exist**, and nothing on the wire could even read
the layered configuration model ADR-0007 built. The model was already
there, complete: global defaults layered under per-server overrides with
**per-field provenance**, "so the UI never has to guess where a value
came from" — a sentence written for exactly the pages that did not yet
exist. The spawner already consumed every field (java path, heaps, JVM
args, timeouts, the jar); the scheduler already consumed retention. The
missing piece was the wire and the three surfaces.

The scoping preamble still holds: the AI part stays ignored, its rooms
kept. Nothing here reaches Dutchmen behavior (§39 lists it; it stays a
reserved row until the AI slice is wanted).

## Decision

### The wire: `config.get`, `config.set`, `network.status` (§7g)

`config.get {serverId}` answers the **effective view** — what the server
actually runs with, after layering — plus the **provenance map** (one
`global`/`custom` word per field) plus the two non-layered fields the
per-server side owns: `displayName` (the registry's) and `jar` (the
per-server file's). `network.status` is a separate method because it is
**time-sensitive** in a way config reads are not: it names the desired
port (the config model), the `server.properties` port (the boot
authority), a **live bind-test** of the effective port, and the other
managed servers whose *desired* port equals this one's. Availability is
a moment-in-time read, never a guarantee — the final authority is the
server binding at boot, and the UI copy says so in those words.

### The patch is tri-state, because "unset" must be expressible

A settings form that can *set* must also be able to *clear back to the
global default*. Absent/keep vs null/clear vs value/set is exactly three
states, so the patch is `Option<Option<T>>` on the Rust side — with a
`tri_state` deserializer, because serde collapses a JSON `null` into the
same `None` as absence and "clear this override" would be silently
indistinguishable from "don't touch it". `displayName` is the one
exception: a server always has a name, so it has no clear state. An
empty patch is a typed `PROTOCOL_INVALID_REQUEST` refusal, not a silent
success.

Validation happens before anything touches disk, with the **field
named** (`CONFIG_INVALID`, context `field`): port 1024–65534, memory
16 MiB–1 TiB, timeouts ≤ 24 h, retention ≤ 1000, Java major 8–100, and
the min ≤ max pair rule checked against the **post-patch file** so a
single set that lands min above max is refused as a whole. The jar
override obeys the same relative-path rule the spawner and the publish
selection enforce. A corrupt config file is a loud typed error, never a
silent reset to defaults — the effective view must not lie.

### A running server is never interrupted from these pages

Overrides are read by the actor at spawn time. Editing memory or the
port of a Running server therefore *cannot* touch the live process —
§60's "reload must not mean restart" holds by construction, and the UI
labels the timing honestly: "the next time the server starts". The port
change propagates through the existing reconciliation (the daemon points
the stamped `server.properties` at the desired port at boot); the
Network page reads `server.properties` for display and **never writes
it behind the server's back** (ADR-0007: the file is Minecraft's).

### The panel: provenance on every row, reserved rooms stated

The three surfaces lazy-load into their own chunks, like the console —
the workspace chrome must not pay for forms a tab may never open.

- **Startup (§38)** renders every layered field with its provenance word
  and a clear-to-global affordance on custom rows. The save **diffes
  against the rendered baseline**: a field the operator never touched
  must not become an override, because the inherited global value shown
  in the form is not their choice. The composed command line
  (`java -Xms… -Xmx… <args> -jar <jar> nogui`) is always visible —
  §38's rule that advanced users can see the actual startup
  configuration, and nobody has to understand JVM command lines to use
  the page.
- **Network (§37)** pairs the desired port (editable, layered) with the
  properties authority and bind address (read-only rows), the probe dot
  (available / in use / nothing to probe — all three states honest),
  a Check-again button, and the conflict list.
- **Settings (§39)** holds the identity (name, join address) and backup
  retention, and states the four rooms the model does not have yet —
  icon, restart policy, crash policy, log retention — as **reserved**
  rows with a note, per §82: unavailable is stated, never faked. A
  rename refreshes the panel's server record through the ordinary read
  path (`server.get` → upsert), never by patching a second store by
  hand.

### Budgets move deliberately, or they do not move

The bundle gate was at 170.2/170 KB with the slice not yet built. The
lines move **in the same commit as the feature**: total JS 170 → 184 KB
gzip, CSS 12 → 14 KB, with the entry chunk untouched (the three chunks
sum to ~6.5 KB JS). Cold start → interactive stays the real budget; the
total grows because features arrived, not because the boot path
regrew. PERFORMANCE-BUDGETS.md carries the rationale.

## Consequences

- The §23 sidebar is complete: Console, Files, Schedules, Network,
  Startup, Settings — with the panel's additional surfaces (metrics,
  logs, players, plugins, backups) alongside.
- `zamin config show/set` and `zamin network status` make the same
  surface scriptable over SSH; the CLI's composed-command preview
  mirrors the panel's.
- `config.set` joins the audited mutations: configuration changes are
  evidence, like every other write.
- The reserved rooms (icon, restart/crash policy, log retention,
  environment variables, Dutchmen behavior) are named on the pages
  themselves, so the next session's founder-vision diff starts from an
  honest list.
- Testing: tri-state codec tests (protocol), the layering/clear/refusal
  e2e trio (daemon), the CLI round trip with a real bind-test and a real
  conflict, and 18 panel tests covering provenance, the baseline diff,
  the probe, and the reserved rooms.
