# ADR-0031 — The extensions foundation: declared, never executed (§56/§57)

**Status:** Accepted · **Date:** 2026-10-09

## Context

§56 asks for an extension/addon system and immediately puts two fences
around it: "Extensions should NOT receive unrestricted access to the
machine" and "Create a permission model." §57 adds the shape: context
menu entries, declared permissions, isolation. The temptation is to
build the fun part first — extension code running somewhere — and bolt
permissions on after. That ordering is how permission models become
rubber stamps: the capabilities exist before the vocabulary that would
constrain them, so every later restriction is a breaking change against
installed extensions.

The panel's other rule points the same way: §82 (unavailable, never
pretend). A room that claims extensions work when nothing isolates them
is a lie; an inventory that honestly says "this is what is installed,
this is what it claims, and nothing executes yet" is not.

## Decision

**Land the declaration half first, and make it the whole contract:**

1. **An extension is a folder with a manifest, nothing else.**
   `<data>/extensions/<name>/zamin-extension.toml` declares `id` (slug
   rules, `[a-z0-9][a-z0-9_-]{0,63}`), `name` (≤ 64 chars), `version`
   (≤ 32), an optional `description` (≤ 200), and a `permissions`
   list. No code ships in the folder; nothing in it is ever executed.

2. **The permission vocabulary is closed and deny-by-default, in two
   families.** `contribution:*` names what the extension may ADD once
   the contribution model lands (context-menu, sidebar-page,
   config-editor, server-integration, publish-provider,
   software-support, dutchmen-tool, marketplace). `data:*` names what
   machine state it may touch (servers read/control, files read/write,
   console read/send, players read/control). A permission string that
   is not in the vocabulary is a manifest rejection — `data:*` and
   invented scopes are refused, never shrugged in. A permission not
   declared is never granted.

3. **The inventory answers for everything it saw.** `extensions.list`
   walks one level (deterministic order, symlinks never followed per
   ADR-0009) and returns valid manifests plus, in-band, a `problems`
   list naming every folder that could not be read and why. A broken
   manifest is evidence on the page, not a silent skip. The result
   carries `contributionsActive: false` so no client can overpromise.

4. **The room is live in the panel now.** `zim://extensions/`
   is a typed destination (§58), in the palette and the tab strip;
   the page renders declarations with `data:*` permissions visually
   apart from `contribution:*` ones, names the problems, states where
   extension folders live, and says in its own render that nothing
   contributes yet. The CLI prints the same inventory
   (`zamin extensions`).

5. **The execution/contribution model is the reserved next room**, and
   landing it will mean: a real isolation boundary (a separate process
   or a scripted host with no direct daemon wire access), grants that
   are per-permission and revocable, and contribution points that can
   only be reached through the typed surfaces this vocabulary already
   names. The vocabulary above is the contract those future
   contributions are judged against — that is why it lands first.

## Consequences

- Operators can install and inspect declarations today; the page and
  the CLI tell the truth about what does and does not happen.
- When the execution model lands, no existing extension needs a
  manifest migration — the manifests validated today are the ones that
  will be granted tomorrow, scoped exactly as declared.
- The wire grows one read-only method (`extensions.list`); the audit
  trail is unaffected because a read is not a mutation.
- The panel's CSS budget was negotiated 19 → 20 KB, in writing
  (PERFORMANCE-BUDGETS.md), for the room's own lazy-chunk stylesheet.
