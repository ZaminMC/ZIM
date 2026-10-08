# ADR-0022 — Specialized editors: the registry and the scoreboard (§34)

**Status:** Accepted · **Date:** 2026-10-08

## Context

ADR-0021 reserved §34's room: specialized config editors were deferred
with the compose row model named as their seam. The founder's example is
the scoreboard — configuration on the left, a live scoreboard preview on
the right, growing out of an extensible editor API rather than a pile of
hardcoded plugin handlers. The rule is explicit: do not hardcode every
plugin; create an extensible editor API.

The seam was real. Compose edits a line-preserving AST of the file — one
pair rewritten, every untouched line kept — so any specialized view that
edits through the same rows inherits the byte-stability guarantee for
free. What was missing was the routing (which file gets which editor)
and the first editor that proves the shape.

The scoping preamble holds: the AI rooms stay reserved, untouched.

## Decision

### A registry, not a switch statement

`app/editor/registry.ts` is the whole routing story: a list of
`SpecializedEditor { id, label, strength(path, text) }` declarations,
and `specializedEditorFor(path, text)` — strongest bidder wins. Detection
is honest in both directions: an editor only claims a file its own model
can parse back (the scoreboard detector literally reads the rows before
claiming the file), and `null` means the generic Compose view answers —
the common case for every `server.properties` and plugin config the
registry cannot speak for. Detection runs once per file open, never per
keystroke: a draft passing through an empty title mid-edit must not yank
the mode away.

The chat, tablist, and server-list editors the founder names are now an
entry away, and the shared dialect they will all preview through already
exists (`app/editor/minecraft.ts`).

### The scoreboard editor rides the row model, not beside it

`app/editor/scoreboard.ts` is a dialect over the properties AST, not a
second representation. The file stays a properties file; the editor
extracts a `ScoreboardSpec` (title + rows), and every write goes back
through the same lines. Two layouts the wild actually speaks are both
editable:

- **single** — one pair carries the rows (`lines=a,b,c`); the editor
  splits and joins that one pair's value;
- **indexed** — one pair per row (`line.1=…`, `line-2=…`, separators
  vary); the editor keeps each row PINNED to its key, so removing a row
  drops exactly that pair (a deliberate structural edit), adding one
  appends the file's next index in the file's own key style, and
  reordering swaps the two values.

Values re-encode in the canonical properties form the compose pipeline
already writes; comments, blanks, ordering, and foreign keys keep their
bytes. Placeholders (`%online%` and friends) render literally — they
belong to the plugin, and the preview does not pretend to resolve them.

### The preview is the client's palette, nothing invented

`app/editor/minecraft.ts` parses `&`/`§` legacy codes into styled
segments using the client's actual 16-color palette. Unknown codes stay
in the text — the preview never silently eats characters the author
typed. The scoreboard's live pane renders those segments in a replica
of the in-game sidebar (title centered, rows right-aligned on the dark
strip), so what the operator types is what a player sees.

### Three faces, one pipeline

The files editor grows a third mode tab when the registry claims the
open file — Source, Compose, then the specialized editor's label. All
three are faces over one `composeFile` AST; the specialized mode is not
a separate save path, it edits through the same `composeApply`
transform, so Save, Save & Restart, and the §35 reload note apply
unchanged.

## Consequences

- `registry.ts` + `scoreboard.ts` + `minecraft.ts` + the scoreboard
  component ride the already-lazy files chunk; the entry chunk is
  untouched (cold start keeps its budget).
- A new specialized editor is one registry entry with a strength
  function and a component that edits through the rows — no FilesView
  surgery, no hardcoding.
- The single-pair layout rewrites its one pair on ANY row edit (that is
  where the rows live); the indexed layout touches only the pairs the
  operator named. Both are honest about which file shape they speak,
  and the editor says so in its footer note.
- The `withEditors` test seam refills the registry for a test and
  restores it afterwards — no test can leave the panel rerouted.
