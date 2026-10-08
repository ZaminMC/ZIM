# ADR-0033: the browser shell becomes a Chromium-derived architecture in stages

Date: 2026-10-09 · Status: accepted · Deciders: P0.2 mandate (product owner)
Related: ADR-0032 (keyboard contract), ADR-0030 (motion law), ADR-0029
(versioned channel), ADR-0024 (development channel) · Investigation:
`../browser-shell/architecture-investigation.md`

## Context

P0.2's first pass delivered keyboard conformance and backend fixes — good
work, wrong conclusion. The browser UI itself is still a custom React shell
in a single webview: tabs are React state, pages are React components, and
no amount of shortcut polish changes that. The mandate: investigate
Chromium's actual implementation (`chromium/chromium`) and correct the
architecture, not the behavior copy.

## Options compared (full matrix in the investigation)

A — current Tauri+React single-webview shell · B — Tauri restructured
(per-tab webviews, Rust tab model, ported chrome) · C — CEF-based shell ·
D — Chromium-derived fork (Vivaldi model) · E — Electron / real-Chrome
embedding / servo-class engines.

Key facts from the source inspection:

- Chromium's chrome is built on Views/aura and compiles only inside a
  Chromium build — R1 reuse is fork-only.
- Everything the chrome *does* — the 38-line tab width law, the drag state
  machine (10 DIP start, 15 DIP detach magnetism), TabStripModel's insertion
  and pinned-block policy, omnibox State/keyword machine, BookmarkModel's
  permanent-node/UUID/codec design, the 443-command ID space, session
  command logs — is BSD-3 and **source-portable today** (R2) with
  attribution and without Chrome branding.
- CEF ships content, not chrome. Electron ships content, not chrome. Only
  the fork ships chrome — at the cost of owning Chromium security cadence,
  a build farm, and a 100–200 MB binary before any ZaminPanel feature.

## Decision

1. **Phase 1 (P0.2 completion)** — restructure to **Option B**: the React
   tab strip dies; a Rust tab model (TabStripModel semantics) owns tabs,
   windows, selection, groups, pinned state; one webview per tab, visible
   one at a time; the React document is demoted to the chrome layer
   (tab strip / toolbar / bookmarks / menus) and renders from the model
   over IPC. Drag/tear-off follow TabDragController's state machine and
   thresholds. All ports carry upstream citations; conformance tests assert
   our metrics equal upstream's constants.
2. **Phase 2 gate** — if WebKitGTK/WKWebView content quality blocks real
   usage, swap the content engine to CEF behind the same model. Platform
   swap, not rewrite.
3. **Phase 3 gate** — adopt the **Chromium fork** when at least three of:
   extension-parity demand from paying/design partners; a dedicated
   Chromium-infra role is fundable; user scale where engine parity is the
   measured blocker; build/sign/release farm budget. Until then D is the
   documented destination, not the next sprint.

## Consequences

- The product gains a real tab engine whose behavior is defined by upstream
  source, with mechanical conformance proof — "Chromium-derived" becomes a
  test result, not an adjective.
- We explicitly accept Phase 1's honest limits: no per-site process
  isolation matrix; tear-off re-embeds content (reload) rather than moving
  a live WebContents; renderer divergence on Linux/macOS until Phase 2.
- Feature freeze holds until Phase 1's shell restructure lands; §56–§64
  feature work stays parked.
- Backend P0 work (console, wire, resolver, spawn, evidence, keyboard
  contract) survives untouched — it is all process-side.
- Licensing: BSD-3 attribution ledger (`CHROMIUM-PORTS.md`, shipped
  CREDITS) is a release-gate artifact; no Chrome assets or trademarks.
- The keyboard contract (ADR-0032) re-bases onto Chromium's command ID
  space (`chrome_command_ids.h`) — one table for commands, keys, and tests.
