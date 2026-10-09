# ADR-0028 — The feedback lane (§84's room, named)

**Status:** Accepted · **Date:** 2026-10-08

## Context

The operator needs a way to tell the builders what broke without leaving
the product: type what happened, optionally attach a screenshot, and have
a real GitHub issue land in the development repository. The constraints
are the founder's, applied to our own lane:

- §47's credential discipline: a token must never ride into an issue
  body, a URL, a log line, or an error sentence.
- §82's no fake functionality: both routes must be real, and the page
  must say which one it is taking before anything leaves.
- §81's error bar: a failed send is a typed state with the report kept,
  never a silent loss and never a dead end.
- GitHub's issues API cannot attach images. Any design that claims to
  "upload" a screenshot through the API is lying.

## Decision

**Two honest routes, stated on the page before sending.**

1. **Token route.** The operator configures a GitHub token in Settings
   (masked field, machine-local storage, verified against `GET /user`
   which names the account). Send POSTs to
   `repos/ZaminMC/ZIM/issues` with the `feedback` label; a 422
   about the label retries without it (the report still ships); 401 marks
   the sign-in invalid and keeps the text; 403 and other failures answer
   as typed notes with the report preserved. Success offers the issue
   URL, opens it in the browser, and — when a screenshot was pasted —
   copies it back to the clipboard, because the API cannot carry it; the
   issue page takes a paste.
2. **Browser route (the default; no token needed).** Send composes the
   issue body (the operator's words plus a small diagnostics block:
   version, platform, screenshot truth), prefills GitHub's own
   `issues/new` form via URL parameters, opens the operator's browser —
   where their GitHub session already lives — and hands a pasted
   screenshot back to the clipboard for GitHub's native paste-attach. An
   over-long body truncates with a sentence that says the tail was cut.

**Screenshot flow.** The operator pastes an image into the page (Ctrl+V,
the same muscle as Win+Shift+S); the page previews it and, on send, hands
it to the clipboard plugin (`writeImage`) for GitHub's form. An image over
8 MB is refused with the reason (GitHub's own paste-attach limit). The
page never uploads the image anywhere itself.

**Diagnostics block.** Every report carries: the installed version (the
host's answer, or an honest "the host has not answered yet"), the
platform, the send route, and whether a screenshot rides along. Nothing
else — no logs, no file contents, no server names; the report is public
by nature and the block is shown in the page before sending.

**The token's life.** Stored machine-local only (the panel's own
persisted settings), rendered masked in Settings, sent ONLY as the
`Authorization` header to `api.github.com`, tested for exactly that in
the suite. It never enters `composeBody`, the browser URL, an error
note, or a log line.

**Surface.** `zim://feedback/` is a typed destination: the ⋮ menu,
the palette, and the about page all reach it. The page is a lazy chunk;
the Settings page moved onto the internal pages' lazy lane in the same
slice, so the entry chunk ended lighter than it started.

## Consequences

- Filing feedback needs no backend service, no issue-creation proxy, and
  no stored org credential — the operator's own token (or their own
  browser session) acts on their behalf, against a public repo.
- The screenshot's honest path is the clipboard; "attach" in the UI
  always says what actually happens. If GitHub ever ships an issues
  attachment API, the page grows it without changing the routes.
- The token is a bearer secret on the operator's machine; the settings
  copy says so, and the panel treats it with §47's discipline. A
  fine-grained token with only "issues: write" is the recommended shape.
