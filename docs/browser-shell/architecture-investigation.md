# Browser-Shell Architecture Investigation

Status: **delivered 2026-10-09** · Scope: P0.2 · Decides: how ZIM becomes a
real browser instead of a React dashboard wearing browser furniture.
Decision record: ADR-0033. Executable port list: `chromium-porting-spec.md`.
Reference sources (fetched, committed): `chromium-ref/` — 46 files from
`chromium/chromium @ main`, plus CEF/wry/Tauri docs.

---

## 0. Mandate

The P0.2 pass shipped keyboard conformance (ADR-0032) and backend P0 fixes —
good work, but it answered a different question. The actual complaint: **the
browser UI itself is wrong.** The shell is a custom React/Tauri browser
imitation. The mandate is to investigate how close we can technically get to
**Chromium's actual browser UI architecture and implementation**, compare
realistic architectures A–E without cherry-picking the easy one, and then
correct the foundation. Feature freeze holds during this.

## 1. Method and sources

Primary reference: the Chromium repository itself. We fetched and committed
the actual implementations (headers + key `.cc` files) under
`docs/browser-shell/chromium-ref/`, organized to mirror upstream:

| Upstream layer | Files inspected | What it proves |
|---|---|---|
| `chrome/browser/ui/tabs/` | `tab_strip_model.{h,cc}` (1551/6166 ln), `tab_style.{h,cc}`, `tab_types.h`, `tab_group_model.h`, `tab_group_visual_data.h` | the tab *model* layer: insertion policy, pinning, groups, opener semantics |
| `chrome/browser/ui/views/tabs/` | `tab.{h,cc}`, `tab_strip.{h,cc}` (481/2554 ln), `tab_strip_controller.h`, `tab_width_constraints.{h,cc}`, `tab_layout_types.h`, `tab_drag_controller.{h,cc}` (760/3350 ln), `tab_group_header.h`, `tab_group_views.h`, `layout_constants.cc`, `new_tab_button.cc` | the tab *view* layer: layout math, drag state machine, group chrome |
| omnibox | `omnibox_edit_model.h` (993 ln), `autocomplete_controller.h`, `autocomplete_input.h`, `autocomplete_match.h`, `omnibox_view_views.h`, `omnibox_client.h`, `omnibox_popup_view_views.h` | omnibox model/view split, input classification, popup presenters |
| bookmarks | `bookmark_model.h`, `bookmark_node.h`, `bookmark_bar_view.h` | bookmark model service + bar view |
| commands/window | `browser.h`, `browser_command_controller.h`, `chrome/app/chrome_command_ids.h` (443 IDC_ defines) | the Browser controller, command routing, command ID space |
| session | `session_service.h`, `session_service_commands.h`, `tab_restore_service.h` | session persistence & closed-tab restore |
| content | `content/public/browser/web_contents.h` (1979 ln) | the per-tab content object that the whole chrome drives |
| ecosystem | CEF `README.md`, docs.rs `tauri::window::Window`, `wry::WebViewBuilder` | what the embedding layers actually expose |
| license | `LICENSE` (BSD-3) | what reuse is legal |

Line counts are upstream counts at fetch time (main branch, 2026-10). We
inspected implementations, not marketing pages.

## 2. Ground truth: what Chromium's browser UI actually is

### 2.1 The layering

Chromium's browser chrome is four layers, and the **browser UI lives in the
top two**:

1. `//content` — `WebContents`: the per-tab content object (navigation,
   render-process lifecycle, etc.). This is the part CEF/Tauri-class embedders
   see.
2. `//components` — model services shared with iOS/Android: `bookmarks`,
   omnibox core (`components/omnibox/browser/*`), `sessions`,
   `tab_groups`.
3. `//chrome/browser/ui/…` — the **browser application layer**:
   `Browser`, `TabStripModel`, `BrowserCommandController`, session glue.
4. `//chrome/browser/ui/views/…` + `//ui/views` + `//ui/aura` — the **Views
   UI toolkit layer** where `TabStripView`, `Tab`, `TabDragController`,
   `OmniboxViewViews`, `BookmarkBarView`, `BrowserView` are drawn.

The decisive fact: **the browser chrome (tab strip, omnibox, bookmark bar,
drag, window frame) is built on the Views/aura toolkit, which is welded into
Chromium's build (GN/ninja, `//base`, `//ui/gfx`, compositor). None of A/B/C
below can compile it. Reuse of *views code* happens only inside a Chromium
build.** Model code (`//components`) is more portable but still depends on
`//base`. This is why "Chromium-derived" has three distinct legal/technical
meanings, defined in §3.

### 2.2 The Browser object (controller pattern)

`Browser` (`commands/browser.h`) is the controller: it owns a
`TabStripModel`, a `SessionID`, the window interface
(`BrowserWindowInterface`), and is constructed from `BrowserInitState`.
`BrowserCommandController` (`browser_command_controller.h`) owns
enabled/disabled state per command and routes execution into the Browser.
UI = `BrowserView`, a pure view that observes the model. This is exactly the
MVC split our ADR-0032 keyboard contract gestures at — Chromium really does
separate model (`TabStripModel`) from view (`TabStripView`) via
`TabStripController` (`tab_strip_controller.h` is the interface both see).

### 2.3 Tab model semantics (TabStripModel, 6166 lines)

Verified API surface and policies:

- **Insertion policy**: "Tabs opened in the foreground inherit the opener of
  the previously active tab" (`AppendWebContents` doc). All inserts funnel
  through `InsertWebContentsAt`, which **enforces the pinned-before-unpinned
  invariant** — it relocates the index if it would break the pinned block.
- **Pinning**: `SetTabPinned(index, bool)` returns the resulting index.
- **Groups**: `AddToNewGroupImpl`, `AddToNewGroupFromContextIndex`,
  `RemoveFromGroup`, group carried as `tab_groups::TabGroupId` on insert;
  visual data (title, color, collapsed) in
  `components/tab_groups/tab_group_visual_data.h`.
- **Movement**: `MoveWebContentsAt(index, to_position, select_after_move)`,
  `MoveTabNext/Previous`, `MoveTabRelative`.
- **Detach**: first-class — `DetachWebContentsAtForInsertion` returns a
  `DetachedTab`/`DetachedTabCollection` (with `TabRemovedReason`,
  detach reason, preserved pinned/group state) designed to be **re-inserted
  into another strip** — this is the tear-off-to-window and drag-between-
  windows substrate.
- **Selection**: `SelectTabAt`, `SelectNextTab`, `SelectPreviousTab`,
  `ui::ListSelectionModel` (multi-select), `closing_all()` optimization
  signal.

### 2.4 Tab layout math (the numbers that make it look like Chrome)

From `tabs/tab_style.cc` and `tabs-views/layout_constants.cc` (DIP values,
our porting spec carries the exact lines):

| Metric | Value | Source |
|---|---|---|
| Standard tab width | **232** (+ 2×12 corner extensions = **256** effective) | `tab_style.cc` kTabWidth |
| Tab height | **34** + 1 toolbar overlap; strip height = 34 + 1 + 6 padding | `layout_constants.cc` |
| Top corner radius | **10** | `tab_style.cc` |
| Bottom corner radius | **12** | `tab_style.cc` |
| Tab overlap | **2×12 − separator width+margins** (computed, not hardcoded) | `tab_style.cc` |
| Pinned width | 24 content + contents insets | `tab_style.cc` |
| Min inactive width | 16 interior + overlap accounting | `tab_style.cc` |
| Min active width | max(favicon 16, close button size) + insets | `tab_style.cc` |
| Separator | 2 wide × 20 tall, corner radius 1 | `tab_style.cc`, `layout_constants.cc` |
| Close button | 16 (14 if rounded-icons) | `layout_constants.cc` |
| Tab pre-title padding | 8; after-title padding 4 | `layout_constants.cc` |

And the layout *algorithm* (`tab_width_constraints.cc`, 38 lines — the whole
file): active vs inactive minimum widths, **pinned ⇒ fixed width, closing ⇒
collapse to overlap width**, and `LayoutDomain` distinguishing "inactive
width below active width" vs "equal" — the state machine that makes Chrome
tabs shrink and stop shrinking the way they do. This file alone is the
difference between "tabs" and *Chromium tabs*.

### 2.5 Drag & tear-off (TabDragController, 3350 lines)

Verified from `tabs-views/dragging/tab_drag_controller.{h,cc}`:

- Created on mouse-press on a tab; `Drag()` per mouse move; drag *session*
  starts only after real movement (`kMinimumDragDistance = 10`).
- `enum class DragState { kNotStarted, kDraggingTabs, kDraggingWindow,
  kDraggingUsingSystemDnD, kWaitingToExitRunLoop, … }` — the honest state
  machine, including the platform forks (client-driven window dragging vs
  system drag-and-drop where the new window stays hidden until drop).
- **Detach thresholds**: `kVerticalDetachMagnetism = 15` DIP (touch: 50);
  maximized-window inset `kMaximizedWindowInset = 10`.
- Detach = `DetachIntoNewBrowserAndRunMoveLoop`: a **new Browser is created**
  and the OS window move-loop takes over — tear-off is not a modal hack, it
  is literally "make a new Browser and keep dragging its window".
- Drag images: the tabs render into drag-image layers during the session.

### 2.6 Omnibox

Model/view split, verified:

- `OmniboxEditModel` (components layer, 993-line header): a `State` snapshot
  (user text in progress, user text, **keyword + keyword placeholder +
  KeywordState**, focus state, current `AutocompleteInput`), an `Observer`
  protocol (`OnSelectionChanged`, `OnMatchIconUpdated`, `OnContentsChanged`,
  `OnCharTyped`) — i.e. the omnibox is a model with popup selection state
  machine, not a styled `<input>`.
- `AutocompleteInput` (`autocomplete_input.h`, 521 ln): URL-vs-search
  classification, scheme parsing, dispositions — the "address bar knows what
  a URL is" brain.
- `AutocompleteController` orchestrates providers into ranked matches
  (`autocomplete_match.h`).
- Views: `OmniboxViewViews` (a `views::Textfield`), popup presenters split
  (`omnibox_popup_view_views` vs WebUI popup) — presentation is swappable
  around the same model.

### 2.7 Bookmarks

`BookmarkModel` (`bookmark_model.h`): a `KeyedService` (one per profile),
**UUID-indexed** (`uuid_index.h` include), `TitledUrlIndex` for search,
permanent nodes, codec-based persistence (`BookmarkCodecTest` include),
observer protocol, `ScopedGroupBookmarkActions` for transactional edits.
`bookmark_bar_view.h` is the Views presentation. The model is reusable in
spirit and directly portable in structure; the view is custom per §3.

### 2.8 Commands

`chrome/app/chrome_command_ids.h` — 443 `IDC_` defines in typed blocks:
window/tab block starts 34000 (`IDC_NEW_TAB 34014`, `IDC_CLOSE_TAB 34015`,
`IDC_SELECT_NEXT_TAB 34016`, `IDC_SELECT_TAB_0..7 34018–25`,
`IDC_DUPLICATE_TAB 34027`, `IDC_RESTORE_TAB 34028`, `IDC_MOVE_TAB_NEXT
34032`, `IDC_CLOSE_TAB_GROUP 34104`, `IDC_MOVE_TAB_TO_NEW_WINDOW 34056`),
bookmarks 35000 (`IDC_BOOKMARK_THIS_TAB 35000`, `IDC_BOOKMARK_ALL_TABS
35001`, `IDC_SHOW_BOOKMARK_BAR 40009`), find 37000, zoom 38000,
`IDC_FOCUS_LOCATION 39001`. `BrowserCommandController` keeps the
enabled/disabled matrix and routes to Browser. Our ADR-0032 keyboard
contract already mirrors this shape — that work stands.

### 2.9 Session & tab lifecycle

`SessionService` records a command log (`session_service_commands.h`) per
window/tab; `TabRestoreService` is the closed-tab/window stack with typed
entries (tab, window) and timestamps — restore ordering, pinned preservation,
and per-tab navigation state are all modeled upstream.

### 2.10 What "one tab" is

`WebContents` (`content/public/browser/web_contents.h`, 1979 ln): one per
tab — owns navigation history, render-process attach/detach, lifecycle
(frozen/discarded) hooks. **In Chromium, tabs are content objects the chrome
drives; they are not DOM elements inside one document.** Our current shell
has this exactly backwards: tabs are React state in one document, and
"pages" are React components (`App.tsx` switches on destination kind;
`TabBoundary` is an error boundary). That single fact is the architectural
root of every "this isn't a browser" symptom.

## 3. What can legally and technically be reused

BSD-3-Clause (`chromium-ref/lic/LICENSE`, Chromium Authors): redistribution
and modification permitted with (1) copyright notice retention, (2) license
text in binaries, (3) no Google/Chromium branding endorsement. **Code and
constants: yes, with attribution. Chrome icons/art/trademarks: no.**

Three levels of "reuse", precisely:

- **R1 — Compile upstream components**: only inside a Chromium build
  (Views/aura/`//base` dependency wall). ⇒ only D reaches R1.
- **R2 — Source-derived port**: translate algorithms/constants/semantics
  from upstream files into our Rust/TS, file-by-file, with the upstream path
  + line cited in the port and a generated `CHROMIUM-PORTS.md` attribution
  ledger. Legal (BSD-3), auditable, and — critically — **testable against
  upstream**: our conformance tests can assert our layout constants equal
  upstream's.
- **R3 — Behavior contract** (what we already had): observe and re-specify.
  Useful, but the weakest form. ADR-0032 lives here.

The investigation's core finding: **R2 is available to us today at every
layer we inspected, and R1 is only available via D.** Everything in §2 above
is R2-portable: TabWidthConstraints is 38 lines; the drag state machine's
detach thresholds are three constants; TabStripModel semantics are policy,
not platform. The parts that do NOT port (Views/aura rendering) are exactly
the parts we replace with ZIM concepts anyway.

## 4. The options

### A — Current: Tauri 2 + React single-webview shell (status quo)

One Tauri window, one WebView2/WebKitGTK/WKWebView instance, one React
document. Tabs = zustand state; pages = React components; isolation =
React error boundaries; `src-tauri` is 218 lines (autostart + daemon
spawn). Keyboard contract (ADR-0032) rides `windowBoot`.

- Chromium code reuse: **none** (R3 only).
- Tab architecture: **none** — React state, no WebContents analog.
- Omnibox: styled input + `commitAddress.ts` dialect router.
- Bookmarks: zustand store.
- Groups/pinned/tear-off: partial/mocked in TS.
- Multi-window: single window.
- Process isolation: none. Renderer isolation: none.
- Extensions: our own manifest system (§56/57) — orthogonal, survives.
- Windows/Linux/macOS: WebView2 / WebKitGTK / WKWebView (engine divergence;
  Linux/macOS content quality is genuinely worse than Chromium).
- Build complexity: lowest. Binary: ~10 MB. Update: NSIS + updater plugin
  (ADR-0024/0029), working today.
- Maintenance: lowest — but the "engine" we're maintaining is the wrong one.
- Integrations: protocol/zamind/ZaminCore/Dutchmen untouched (all content-side).
- Migration cost: 0.
- **Verdict: fails the product requirement. Not an option going forward.**

### B — Tauri restructured: real tab architecture, ported chrome brain

Keep Tauri as window/content host, but restructure to Chromium's shape:

- **Model layer in Rust** (zim-shell): port TabStripModel semantics
  (R2): insertion policy with pinned invariant, opener chain, groups,
  DetachedTab for cross-window moves, selection model, TabRestoreService
  stack. Zustand tabs die; the React document is demoted to the **chrome
  layer only** (tab strip / toolbar / bookmarks bar / menus) and renders
  from the Rust model over IPC.
- **Per-tab content**: one webview per tab (wry child-window embedding;
  Tauri v2 multi-webview exists but is feature-gated `unstable` — the raw
  wry path is the engineering route; WebViewBuilderExtUnix confirmed on
  docs.rs, Windows `new_as_child` via the Windows ext trait). Only the
  selected tab's webview is attached/visible — Chrome's own visible-tab
  model.
- **Drag/tear-off**: port the TabDragController state machine (thresholds:
  10 start, 15 detach magnetism, 10 maximized inset) in Rust; tear-off
  creates a new Tauri window and re-embeds at the tab's URL (WebView2 cannot
  reparent live — honest caveat, mitigated by session-restore semantics
  ported from TabRestoreService).
- Chromium reuse: **R2 at every layer** (the §2 inventory).
- Process isolation: partial — one WebView2 process per app (per user-data
  dir) unless per-tab environments are used (costly); renderer isolation
  between tabs: none at WebView level, but each tab is a separate webview
  document, so a crashed tab is recoverable without a React boundary.
- Multi-window: real (Rust model owns windows; tabs move between them via
  DetachedTab).
- Windows/Linux/macOS: engine divergence remains (WebView2/WebKitGTK/WKWebView).
- Build complexity: medium (custom chrome hit-testing, drag regions, child
  webviews). Binary: ~12–15 MB. Update: unchanged.
- Maintenance: we own a real tab engine — that is the product.
- Integrations: unchanged; content tabs navigate zamin:// + http(s).
- Migration cost: the shell rewrite (TabStrip.tsx → Rust model + chrome
  document; App.tsx → webview host) — measured in weeks, not months, and
  all backend P0 work survives (it's process-side).
- **Verdict: the best shippable step; see §6 recommendation.**

### C — CEF-based shell

CEF (BSD, binary distributions tracking Chromium branches; per its README
its canonical use cases include "a light-weight native shell application
that hosts a user interface developed primarily using Web technologies").

- Chromium code reuse: content layer only — **CEF contains no browser
  chrome** (no tab strip, no omnibox, no bookmarks UI; `chrome/` layer
  excluded). The chrome is still ours to build, exactly as in B.
- Tab architecture: per-tab `CefBrowser` = real Chromium render isolation,
  including Linux/macOS (the one place CEF strictly beats B).
- Process isolation: real multi-process (browser/GPU/renderers).
- Omnibox/bookmarks/groups: same R2 ports as B — CEF changes nothing here.
- Drag/tear-off: better than B (renderers can move between CefBrowsers via
  visual-devtools APIs are limited; in practice same reload caveat).
- Extensions: no Chrome extension APIs (chrome/ excluded).
- Windows/Linux/macOS: **uniform Chromium**.
- Build complexity: high — CEF prebuilts, Rust bindings immature (cef-rs),
  custom message routing, crash handling. Binary: **150–250 MB** runtime.
- Update: we ship the engine; CEF lags Chromium branches; security cadence
  is on us.
- Maintenance: high, permanent.
- Integrations: protocol/transport rework (CEF JS bridge replaces
  Tauri IPC); zamind untouched.
- Migration cost: from B — moderate; from A — large.
- **Verdict: solves engine parity, not the chrome problem. Phase-2
  candidate (see §6), not the answer to P0.2 by itself.**

### D — Chromium-derived fork (the Vivaldi model)

Fork/track Chromium and ship our browser *inside* it.

- Chromium code reuse: **R1 — literal.** TabStripModel, TabStyle, drag
  controller, omnibox, BookmarkModel, sessions, commands: all present and
  modifiable. Precedent: Vivaldi ships a web-technologies chrome inside
  Chromium — meaning our existing chrome investment (HTML/CSS chrome from B)
  can port into this model rather than being thrown away.
- Everything browser-grade for free: groups, pinned, tear-off, omnibox,
  session restore, multi-window, **true per-site renderer isolation**,
  extension APIs (if we open that door), Chrome DevTools protocol.
- Windows/Linux/macOS: official.
- Build complexity: extreme — ~30 GB blobless checkout, GN/ninja toolchain,
  multi-hour builds, CI farm, Windows signing at Chrome's cadence.
- Binary: 120–200 MB installer. Update: **we own Chromium security
  patching on Chrome's release cadence** — this is the real, permanent
  cost of D.
- Maintenance: requires a dedicated infra function before it requires a
  UI team.
- Integrations: zamin:// as a real scheme handler; zamind/ZaminCore via
  native host or in-browser services; Dutchmen features become browser
  features.
- Migration cost: B's chrome and model carry over conceptually; the
  platform work does not (Tauri layer dies).
- **Verdict: the destination architecture; gated (see §6).**

### E — Others investigated and rejected

- **Electron**: bundles Chromium content, same missing chrome as A; Node
  instead of our Rust stack; strictly worse than B for us. Rejected.
- **Embedding real Chrome** (app-mode/CDP automation as the UI): cannot
  redistribute or rebrand Chrome; identity impossible. Rejected.
- **Servo/alternative engines**: no browser-chrome foundation, not
  production at our bar. Rejected.
- **Native-chrome toolkits (egui/slint/win32) for the chrome layer**: the
  chrome itself would need years of polish to stop looking homemade —
  the exact failure we are eliminating. Rejected for the chrome layer;
  native code stays where it belongs: the model/engine side of B.

## 5. Honest limits of each option

- B does not give per-site process isolation; a renderer exploit in one
  webview is contained by the OS process model (WebView2), not by a
  Chromium site-isolation matrix. We say so and keep backend P0
  hardening as the compensating control.
- B's tear-off reloads content (WebView2 reparenting does not exist);
  Chrome moves the live WebContents. Session-restore semantics make this
  acceptable, and it is the single biggest day-one behavioral gap vs
  Chrome in B.
- C and D carry real binary-weight and security-cadence costs that land on
  the release channel forever (ADR-0024/0029 update plumbing grows).
- D before product-market fit would starve every other workstream; the
  investigation refuses to recommend it as the *first* step — see gates.

## 6. Recommendation

**Adopt a staged Chromium-derived architecture with D as the explicit
destination:**

- **Phase 1 — now (P0.2 completion): Option B**, with the porting spec
  (`chromium-porting-spec.md`) as the executable definition. Every chrome
  behavior ships as an R2 port with citation; conformance tests assert our
  metrics/semantics against the ported constants (making "Chromium-derived"
  mechanically verifiable instead of rhetorical). The ADR-0032 command
  contract and backend P0 work survive unchanged.
- **Phase 2 — engine parity gate**: if WebKitGTK/WKWebView content quality
  blocks real users, swap the content engine to CEF (Option C) behind the
  same Rust model. The chrome layer and model are engine-agnostic by
  construction in Phase 1, so this is a platform swap, not a rewrite.
- **Phase 3 — fork decision gate (Option D)**: trigger when ≥3 of: paid or
  design-partner customers demand extension parity; team headcount
  supports a dedicated Chromium-infra role; MAU where engine differentiation
  is demonstrably the blocker; funding for a build/sign/release farm. Until
  the gate fires, D is on the roadmap as destination, not as next sprint.

Why this is not "the easy option": Phase 1 is a rewrite of the shell's
brain — the React tab strip dies, the model moves to Rust, the chrome is
rebuilt to upstream metrics with proof. What we refuse to do is what A
does: keep pretending a React document with tabs drawn on it is a browser.
What we equally refuse is burning the next six months on Chromium build
infrastructure while the product's users have no product.

## 7. P0.2 acceptance mapping (from the mandate)

| # | Criterion | How this investigation + plan satisfies it |
|---|---|---|
| 1 | architecture investigated | §1–§4, primary sources committed |
| 2 | justified vs alternatives | §4 A–E matrix, §5 honest limits |
| 3 | Chromium-derived components wherever practical | §3 R2 doctrine + porting spec (38-line layout law, drag state machine, model semantics, command IDs) |
| 4 | no longer feels homemade | Phase 1 rebuilt chrome to upstream metrics; mechanically tested (§6) |
| 5 | tabs/groups/pinned/drag/windows mature primitives | §2.3–2.5 ports are the definitions of maturity we copy |
| 6 | omnibox browser-grade | §2.6 model ports (State/keyword/classification) |
| 7 | bookmarks browser-grade | §2.7 model port (UUID, permanent nodes, codec) |
| 8 | compact polished chrome | metric table §2.4; chrome doc rebuilt against it |
| 9 | real Windows manual validation | dispatched build + `windows-manual-validation-checklist.md` |
| 10 | visually inspected | manual pass is a human gate — explicitly **not** claimed done by green tests |

## 8. References

- Upstream sources: `chromium-ref/**` (paths mirror chromium/chromium@main;
  each file retains its Chromium Authors header per BSD-3).
- CEF: `chromium-ref/ecosystem/cef-readme.md`.
- Tauri/wry surfaces: `chromium-ref/ecosystem/tauri-window.html`,
  `wry-builder.html`, `wry-index.html` (docs.rs snapshots).
- Porting spec: `chromium-porting-spec.md`. Decision:
  `../adr/0033-browser-shell-staged-chromium-architecture.md`.
- Windows validation: `windows-manual-validation-checklist.md`.
