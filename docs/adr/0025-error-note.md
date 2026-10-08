# ADR-0025 — The shared error note (§81)

**Status:** Accepted · **Date:** 2026-10-08

## Context

The founder's error bar is three-part: the failure reads as a human
sentence ("The server rejected the plugin because the installed server
version is incompatible"), the next step rides it when the protocol
typed one, and technical details remain available behind
`[View details]` — not as a raw code the operator must decode, and not
as a wall of JSON above the fold.

The panel's surfaces half-kept this. Every view rendered its error
through `describeError`, but the call sites kept only `.title` — the
typed `remediation` lines and the structured `context` were translated
and then thrown away. The sentence existed; the remediation and the
technical layer were discarded at fourteen call sites across six
workspace views.

## Decision

`ui/ErrorNote.tsx` becomes the shared inner body of every error alert:
the sentence, the remediation list when the protocol carried one, and
— when a code or a context exists — a closed-by-default `<details>`
disclosure holding the code and the pretty-printed context. The owning
view keeps its own alert chrome (class, `role="alert"`, its verbs);
ErrorNote renders only the content, so no surface re-decides the shape
and every surface grows the disclosure at once.

The migration covers the six workspace views whose errors were bare
strings (Players, Schedules, Network, Backups, Plugins, Startup); their
`error` state widens from `string | null` to `DescribedError | null`,
and local pre-flight sentences wrap as remediation-less described
errors — a sentence with nothing technical behind it renders no
disclosure at all, because a summary that opens onto nothing would be
a fake control (§82). Modal flows with bespoke security handling
(PublishModal's §45 interception, NewServerModal, the palette) keep
their shapes and adopt the note as follow-ups.

## Consequences

- Typed daemon errors now teach: `PLUGIN_INCOMPATIBLE` says what to do
  (install a matching build) and shows the file/server-version context
  on one click.
- The CSS budget rises 15 → 16 KB: one shared stylesheet (~120 gzip
  bytes) bought fourteen error surfaces at once — the alternative was
  fourteen bespoke disclosures, which is exactly the regression the
  design system exists to prevent.
- The updater lane's error copy (ADR-0024) already renders through the
  same discipline — sentences from `updatesSentence`, technical detail
  in the `title` attribute — and can move onto the shared note when its
  chrome migrates.
