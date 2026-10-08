# ADR-0027 — Server discovery (§64)

**Status:** Accepted · **Date:** 2026-10-08

## Context

The founder's discovery room: *the registry can detect running servers,
configured servers, previously opened servers, server directories, and
supported server JARs — and opening a server address resolves against the
known registry where possible.* Until now the panel knew exactly what it
had registered; a machine's other servers (a directory someone set up by
hand, a jar sitting in a downloads folder) were invisible, and the
"double-click a supported jar" story had no finding half.

Two honest sources exist. The registry already answers managed servers
(ADR-0004, with markers binding roots to ids). What was missing is the
scan: a bounded, safe walk of operator-chosen roots that recognizes
server-shaped directories and supported jars without pretending a
filename proves more than it does (§82).

## Decision

**Core (`zamin-core::discovery`).** One bounded scan (`scan_roots`) over
operator roots: two levels deep (the root, its directories, one more —
`~/mc/servers/<name>/` is the deep layout this exists for; a server's own
`world/` and `plugins/` trees are not discovery targets), a 4096-entry
budget with a `truncated` flag that means *stopped early*, symlinks never
followed, hidden and staging entries never candidates (ADR-0009's rules
ride along). A directory containing `server.properties` is a candidate —
read for its `server-port` — and is **not descended into**. Otherwise,
files matching `classify_jar` are `jar` candidates: the family is stated
as filename evidence (`paper`, `fabric`, `forge`, `neoforge`, `vanilla`,
`velocity`, …), installers/clients/sources jars are deliberately not
candidates, and an unknown jar says nothing. A `server.properties`-bearing
directory also reports the first supported jar beside it (the platform
the server most plausibly runs). Roots that cannot be read are named in
the report, never silently dropped.

**Wire (`server.discover`, `discovery.roots.get/set`).** The daemon
merges both sources: registry entries first — running/starting/adopting
above the rest — then directories, then jars, each with `kind`, the live
`state` for managed servers, the port for directories, the family for
jars. The merge never names one server twice: a scanned candidate whose
marker id is registered, whose path is a registered root, or which sits
**inside** a registered root (the managed server's own runtime jar) is
dropped. Identity discipline (§61) applies to discovery too. Scan roots
are operator configuration in the global config file (absolute paths
only — a relative root is a loud refusal; the scan runs with daemon
privileges); the daemon's own instances dir is implicit and only named in
the answer when it exists. `query` filters across id, name, and path.

**Clients.** The CLI speaks both verbs (`zamin discover [query]`,
`zamin discovery add|remove|list`). The panel's new-tab page — the
founder's discovery input — gains the machine's answer below the registry
rows in a following slice; the address dialect already routes free text
there.

## Consequences

- Discovery answers *what is here*, honestly labeled: a `jar` row means
  "a file named like a Paper build", not "a verified server". Opening an
  unregistered directory remains an explicit register step, so nothing
  enters the registry by being merely seen.
- The scan is bounded and never follows links; a hostile directory tree
  costs the budget, not the daemon.
- Roots are machine configuration with daemon privilege reach — the set
  verb refuses relative paths loudly, and the panel's roots editor (next
  slice) must keep that refusal visible rather than sanitizing silently.
