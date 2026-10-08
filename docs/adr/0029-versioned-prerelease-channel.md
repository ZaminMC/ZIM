# ADR-0029 — The versioned pre-release channel

**Status:** Accepted · **Date:** 2026-10-09

## Context

The development channel shipped its first four builds (0.1.4 – 0.1.7)
under one rolling release: the fixed `dev` tag in
`ZaminMC/ZaminPanel-Releases`, whose assets were deleted and replaced on
every run, and whose version was `0.1.<run number>` — a number chosen to
guarantee monotonicity for the updater, not to say anything about what
changed. Three costs surfaced:

- The releases page shows exactly one entry at any moment. The history
  of the channel — what shipped when, with which installer — is
  invisible to the people the channel exists for.
- The version number lies by construction: `0.1.7` looks like "the
  seventh patch on 0.1" when it is actually "the seventh release run",
  and a run that re-publishes a fix indistinguishably from one that
  adds a subsystem.
- The founder's versioning intent (v0.1.0 first usable engine, v0.2.0
  major subsystem additions, v0.9.0 feature complete, v1.0.0 production,
  v1.1.0 backwards-compatible features, v1.1.1 bug fix) cannot be
  expressed by a run counter.

The updater's contract is unaffected by any of this and must not bend:
the installed fleet polls one fixed URL
(`…/releases/download/dev/latest.json`, baked into every build at
ADR-0024) and offers an update only when the manifest's version is
strictly greater than the installed one. So:

- the manifest endpoint must never move, and
- a version, once published, must never be re-published with different
  bytes — the updater would silently no-op it for everyone already on
  that version.

## Decision

**Versioned pre-releases on the releases page; the fixed `dev` tag
becomes the manifest anchor.**

1. **The version is chosen, not counted.** `Release (dev)` takes a
   `version` dispatch input (MAJOR.MINOR.PATCH, a leading `v`
   accepted), validated at the first job. The scheme is the founder's,
   written into the workflow header and this ADR:

   | Version | Meaning |
   |---|---|
   | v0.1.0 | first usable engine (shipped as the rolling 0.1.x dev builds) |
   | v0.2.0 | major subsystem additions |
   | v0.9.0 | feature complete / compatibility testing |
   | v1.0.0 | production release |
   | v1.1.0 | backwards-compatible features |
   | v1.1.1 | bug fix |

2. **Every release is its own entry.** The publish job creates
   `vMAJOR.MINOR.PATCH` in the releases repo, marked `--prerelease`,
   carrying the signed installers, the portable archives, and that
   release's own `latest.json`. The manifest's download URLs point at
   the versioned tag, so an update is always fetched from the release
   it belongs to.

3. **The `dev` tag is the anchor, forever.** After publishing the
   versioned release, the workflow rewrites the anchor's `latest.json`
   in place (`--clobber`) — same URL, new contents — and removes the
   anchor's stale installer assets so the entry says honestly: "this is
   the manifest the fleet polls; the installers live in the versioned
   pre-releases."

4. **The workflow refuses to lie to the updater.** Unless
   `allow_republish` is set, the publish job fetches the anchor's
   current manifest and fails the run when the requested version is
   already published (a re-publish would no-op for the fleet) or lower
   than it (the updater would correctly ignore it), and when the
   `v`-tag already exists. The honest fix is always "bump the patch";
   `allow_republish` exists for re-rolling a release that shipped
   broken and nobody may have it yet.

5. **Pre-release, on purpose.** Every entry on this channel is marked
   pre-release until v1.0.0: this is a development channel, and the
   label should say so everywhere a release can be seen.

## Consequences

- The releases page becomes the channel's changelog: one entry per
  release, newest first, each naming its subsystem additions in the
  title and notes.
- A broken release can be re-rolled only with `allow_republish`, and
  only for the versioned entry — the anchor is rewritten either way, so
  even a re-roll is one updater cycle from the fleet.
- The old `dev` release's installer assets (0.1.4 – 0.1.7) disappear
  from the anchor the next time it is refreshed; they remain reachable
  in principle through nothing. This is accepted: the anchor's job is
  the manifest, and the fleet updates past those builds on its next
  cycle.
- `0.1.<run number>` is retired. The first chosen version continues
  from the fleet's installed base (0.1.7) with **v0.2.0** — the
  feedback lane (ADR-0028) and server discovery (ADR-0027) are major
  subsystem additions by the scheme's own definition.
