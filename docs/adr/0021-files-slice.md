# ADR-0021 — The files slice: copy, search, and the editor's two modes (§31–§35)

**Status:** Accepted · **Date:** 2026-10-08

## Context

The founder's file manager (§32) lists more than the file browser already
did: search, upload, download, move, and copy belong to the page, and the
editor (§33) owes two modes — Source showing the real file, Compose
rendering friendly controls — with §34's specialized editors growing out
of an extensible shape and §35's reload action knowing how a saved file
reaches the server instead of making the operator type `/reload` and
hope. Metrics (§31) wanted its rows to carry both sides of a ratio:
current and allowed.

The filesystem machinery was already strict (ADR-0009): rooted, canonical,
symlink-denied, chunked. What was missing was verbs and knowledge.

The scoping preamble holds: the AI rooms stay reserved, untouched.

## Decision

### Copy never overwrites, and proves it can finish before it starts

`files.copy {serverId, from, to}` copies a file or a whole tree. Three
rules make it safe to put behind one click:

- **A pre-flight measure pass** walks the source (same symlink and depth
  rules as the copy) and totals the bytes before the first byte moves.
  A copy that would exceed the budget (`FS_COPY_TOO_LARGE`) is refused
  with nothing at the target — a half-copied `plugins/` is worse than a
  refusal, because it looks like success. The budget is 2 GiB per call;
  bigger trees belong to backups, which stream.
- **The target must not exist** (`FS_COPY_TARGET_EXISTS`). The panel's
  answer to a refusal is a fresh suggested name, not an overwrite; a
  silent replacement is the one mistake a rename cannot undo.
- **Symlinks inside the tree refuse the whole copy**
  (`FS_SYMLINK_REFUSED`). Duplicating a link's target silently duplicates
  a subtree the operator did not point at; following one out of the root
  is the thing ADR-0009 will never do. A symlink path as the top-level
  source resolves like every read — the copy lands the target's bytes
  under the link's name, exactly what a read of that path answers.

`FS_TOO_DEEP` covers trees past the walk's 32-level bound.

### Search is a bounded walk that says when it truncated

`files.search {serverId, query}` walks the whole root matching names
case-insensitively, answers sorted, and reports `scanned` (entries
visited) plus `truncated` (the 200-hit cap or the 32-level depth bound
cut the answer short). No index, no persistence — a bounded walk a few
times a minute is cheaper than a cache that can lie. The daemon's own
staging directory is invisible to the search: an answer about the
server's files never includes daemon plumbing. An empty query is a
protocol error — the listing, not the search, is how one sees a
directory.

### The editor keeps one truth: a line-preserving properties AST

Compose (§33) is a view, never a second representation. The parser keeps
every raw line — comments, blanks, spacing, order — and rewrites only the
pairs the operator touched, so a save cannot disturb a byte it was not
handed. A pair's control kind comes from its own bytes (`true`/`false`
is a switch, an integer is a number field, the rest is text): no
hardcoded key list to drift out of date, unknown keys still editable.
YAML stays Source-only for now; a YAML compose mode wants a real AST
round-trip (comments included) and its own decision — the room is
stated, not faked (§82).

### §35: the system knows how the file loads, and it never says /reload

`reloadPlanFor(path)` maps the file's position in the server's life to
one honest sentence and one verb: `server.properties` and the JVM config
family (bukkit/spigot/paper/purpur/folia yml) are read at boot — Save &
Restart applies them; plugin configs say the panel does not fake a plugin
reload — Save & Restart applies them for real; files with no story get
none. Bukkit's `/reload` is deliberately absent everywhere: it is the
one verb that pretends (half-applied configs, leaked memory). The
restart button exists only while the server is actually running, saves
first (bytes land, then boot), and confirms before disconnecting
players.

### Metrics rows carry their ceilings

§31's current/allowed is three ratios, each ceiling owned by the thing
that actually decides it: CPU against the webview host's
`hardwareConcurrency`, memory against the layered config model's -Xmx
(ADR-0007/0019), players against the server's own max-players via Server
List Ping, re-asked on the ping's clock. A ceiling nobody has named
renders no meter — the panel never invents a scale (§82).

## Consequences

- The wire grows two methods (`files.copy`, `files.search`) and four
  error codes; both clients (panel, CLI) reach the same surface, and the
  CLI's `files` group (ls/find/cp/mv/mkdir/rm/get/put) proves the verbs
  over the real daemon.
- Upload/download are staged/chunked reads in both directions; the
  panel's 100 MiB download cap sends bigger trees to backups instead of
  crashing a tab.
- §34's specialized editors (scoreboard previews and friends) are
  deferred with their room reserved: the compose row model is the
  extensible seam they will plug into, and nothing here hardcodes a
  plugin.
