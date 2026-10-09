# Chromium Porting Spec — Browser Shell (P0.2 Phase 1)

Status: executable definition of Option B (architecture investigation §6).
Every row carries its upstream citation. Ported code must carry the upstream
copyright header (BSD-3) and an entry in `CHROMIUM-PORTS.md` (attribution
ledger, generated at build time into CREDITS). Upstream refs =
`chromium/chromium @ main`, mirrored in `chromium-ref/`.

Port levels: **M** = port model semantics (Rust, `crates/…/shell/` or
`apps/panel/src-tauri/src/shell/`), **V** = port view metrics (CSS/TS chrome
layer), **B** = port behavior contract (tests assert both sides).

---

## 1. Tab strip layout (the numbers Chrome uses)

Source: `chrome/browser/ui/tabs/tab_style.cc`, `chrome/browser/ui/layout_constants.cc`,
`chrome/browser/ui/views/tabs/tab_width_constraints.{h,cc}`.

| Constant | Value (DIP) | Upstream symbol | Level |
|---|---|---|---|
| Standard tab width | 232 content (+2×12 corner extensions ⇒ 256 effective) | `kTabWidth`, `GetStandardWidth()` | V+B |
| Standard tab height | 34 (+1 toolbar overlap); strip = 34+1+6 | `kTabHeight`, `kTabstripToolbarOverlap=1`, `kTabStripPadding=6` | V+B |
| Top corner radius | 10 | `GetTopCornerRadius()` | V |
| Bottom corner radius | 12 | `GetBottomCornerRadius()` | V |
| Tab overlap | `2×12 − (separator width 2 + margins)` | `GetTabOverlap()` | V+B |
| Pinned width | 24 content + contents insets | `GetPinnedWidth()`, `kTabPinnedContentWidth` | V+B |
| Min active width | max(favicon 16, close 16/14) + insets | `GetMinimumActiveWidth()` | V+B |
| Min inactive width | 16 interior + overlap accounting | `GetMinimumInactiveWidth()` | V+B |
| Separator | 2 × 20, radius 1, horiz margin 2 | `kSeparatorThickness/Height`, `GetSeparatorSize()` | V |
| Close button | 16 (14 rounded-icons) | `kTabCloseButtonSize` | V |
| Title paddings | pre 8, after 4 | `kTabPreTitlePadding`, `kTabAfterTitlePadding` | V |
| New-tab button | per `new_tab_button.cc` preferred size | fetch-time check | V |
| Location bar height | 34 (touch 36) | `kLocationBarHeight` | V |
| Toolbar element padding | 4 | `kToolbarElementPadding` | V |
| Bookmark bar | height = button height + padding | `kBookmarkBarHeight` | V |

Layout law (port whole algorithm, `tab_width_constraints.cc`, 38 lines):

```
GetMinimumWidth()      = active ? min_active : min_inactive
GetPreferredWidth()    = standard_width
closed  ⇒ width        = tab_overlap
pinned  ⇒ width        = pinned_tab_width        (fixed)
domain  = inactive_width_below_active | inactive_width_equals_active
```

**Acceptance**: a conformance test asserts our chrome's rendered tab widths
for {active, inactive, pinned, closing} equal these constants at zoom=1 —
the same numbers, not "similar" numbers.

## 2. TabStripModel semantics (model layer, Rust)

Source: `chrome/browser/ui/tabs/tab_strip_model.{h,cc}`.

| Behavior | Semantics to port | Upstream symbol | Level |
|---|---|---|---|
| Insertion | foreground tab **inherits opener** of previously active tab; all inserts funnel through one entry point | `AppendWebContents` doc, `InsertWebContentsAt` | M |
| Pinned invariant | pinned block precedes unpinned; insert relocates index to preserve it | `InsertWebContentsAt` note | M+B |
| Pin/unpin | returns resulting index; block-border swap on pin/unpin | `SetTabPinned` | M |
| Move | `MoveWebContentsAt(index, to, select_after_move)`; `MoveTabNext/Previous` | same | M |
| Selection | `SelectTabAt/Next/Previous`; multi-select model; `closing_all` signal | same | M |
| Groups | group id carried on insert; `AddToNewGroupFromContextIndex`, `RemoveFromGroup`; visual data (title/color/collapsed) as data struct | `AddToNewGroupImpl`, `tab_group_visual_data.h` | M+B |
| Detach | `DetachedTab` struct: original index, pinned-at-removal, remove reason — designed for reinsertion into another strip | `DetachWebContentsAtForInsertion` | M |
| Close | close types / close-all semantics with `closing_all` observer opt-out | `CloseWebContents(es)` | M |

## 3. Drag & tear-off state machine (Rust host + chrome layer)

Source: `chrome/browser/ui/views/tabs/dragging/tab_drag_controller.{h,cc}`.

- States: `kNotStarted → kDraggingTabs → kDraggingWindow` (+ platform forks
  `kDraggingUsingSystemDnD`, `kWaitingToExitRunLoop`) — port the state set,
  name the states identically (B test: state transitions asserted by name).
- Thresholds: drag session starts after **10 DIP** (`kMinimumDragDistance`);
  detach magnetism **15 DIP** vertical (`kVerticalDetachMagnetism`, touch
  50); maximized-window detach inset **10** (`kMaximizedWindowInset`).
- Detach = create a **new window (Browser)** and continue the drag in it
  (`DetachIntoNewBrowserAndRunMoveLoop`); on drop outside any strip the
  window stays where dropped.
- Reorder = attached move with the §1 width law animating neighbors.
- Level: M (thresholds+states) + B (JS-side hit-testing mirrors the same
  constants; single source of truth in Rust, chrome reads via IPC).
- Caveat to ship honestly: tear-off re-embeds content at the tab's URL
  (WebView2 reparenting does not exist); restore semantics per §5 mask it.

## 4. Omnibox (chrome layer + Rust classification)

Source: `components/omnibox/browser/omnibox_edit_model.h`,
`autocomplete_input.h`, `autocomplete_controller.h`, `omnibox_view_views.h`.

- Model/View split: keep ours honest — an edit **model** with `State`
  (user_text, keyword, KeywordState, focus_state) and observer events
  (`OnSelectionChanged`, `OnContentsChanged`, `OnCharTyped`); the styled
  input is a view of it. Level M(TS)+B.
- Input classification: port `AutocompleteInput`'s decision shape — scheme
  detection, "URL vs search" heuristics, dispositions (current-tab /
  new-tab / new-window). Zamin dialects (`zamin://`, server refs) extend the
  classifier; they do not fork it. Level M+B.
- Popup: selection state machine with wrap-around, revert-to-original on
  Esc, per-match metadata model (as in `autocomplete_match.h`). Level B.
- Keyword mode (engine-name mode): port the state machine shape
  (keyword → placeholder → accepted). Level B.

## 5. Session & closed-tab restore (Rust)

Source: `chrome/browser/sessions/session_service.h`,
`components/sessions/core/{session_service_commands.h,tab_restore_service.h}`.

- Command-log persistence: window/tab events recorded as ordered commands
  (create/close/select/navigate), replayed at boot. Level M.
- Closed-tab stack: typed entries (tab vs window), newest-first, pinned
  flags preserved, `ReopenClosedTab` pops correctly across windows. Level
  M+B.
- Boot restore: "continue where you were" per ADR-0029 channel semantics —
  restore ordering respects window/tab structure.

## 6. Bookmarks model (Rust + chrome view)

Source: `components/bookmarks/browser/bookmark_model.h`, `bookmark_node.h`,
`chrome/browser/ui/views/bookmarks/bookmark_bar_view.h`.

- Model: permanent nodes (bar / other), UUID-keyed, observer protocol,
  transactional bulk edits, codec-based JSON persistence. Level M.
- View: bar reads the model only via observers; show/hide is a command
  (`IDC_SHOW_BOOKMARK_BAR 40009`), not a layout accident. Level V+B.

## 7. Command IDs (align our contract to Chromium's)

Source: `chrome/app/chrome_command_ids.h`. ADR-0032 keyboard contract maps
onto Chromium's ID space — adopt the numbering as our command registry
constants so conformance tests cite one table:

```
NEW_TAB 34014 · CLOSE_TAB 34015 · SELECT_NEXT_TAB 34016 ·
SELECT_PREVIOUS_TAB 34017 · SELECT_TAB_0..7 34018–25 ·
DUPLICATE_TAB 34027 · RESTORE_TAB 34028 · MOVE_TAB_NEXT 34032 ·
MOVE_TAB_PREVIOUS 34033 · MOVE_TAB_TO_NEW_WINDOW 34056 ·
CLOSE_TAB_GROUP 34104 · BOOKMARK_THIS_TAB 35000 · BOOKMARK_ALL_TABS 35001 ·
FIND 37000 · ZOOM_PLUS/NORMAL/MINUS 38001–3 · FOCUS_LOCATION 39001 ·
SHOW_BOOKMARK_BAR 40009
```

Level B (already half-built in ADR-0032 work; re-base onto these IDs).

## 8. Attribution ledger (license compliance)

- Every ported file header: `// Ported from chromium/chromium <path>@<rev>
  under BSD-3-Clause. Ported 2026-10 by ZIM contributors.`
- `CHROMIUM-PORTS.md` at repo root: table of ports ↔ upstream paths ↔
  revision. Generated CREDITS ships in the NSIS/AppImage payloads.
- No Chrome artwork, icons, or trademarks are ported. All chrome visuals
  are ZIM-authored to the §1 metrics.

## 9. Non-goals for Phase 1

- Chrome extension APIs (§56/57 manifest system remains ours).
- Per-site process isolation matrix (documented limit, §5 of investigation).
- Chromium fork infra (gated per investigation §6).
