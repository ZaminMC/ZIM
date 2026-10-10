# Chromium backport — provenance ledger

The strip's tab visuals are **Chromium's own implementation, ported** — not
a lookalike. This directory (`ui/chromium/`) is the single source of truth
for that law; `frame.css` and `FrameApp.tsx` only consume it.

- **Upstream**: chromium/src `main` @ **157.0.8097.0**, fetched 2026-10-10
  from chromium.googlesource.com.
- **License**: BSD-style, Copyright The Chromium Authors —
  `LICENSE.CHROMIUM` in this directory.

## What was ported, file by file

| Upstream file | Ported into | What it governs |
| --- | --- | --- |
| `chrome/browser/ui/layout_constants.{h,cc}` | `chromiumTabs.ts` (layout constants) | kTabHeight 35, kTabStripPadding 6, band 41, paddings, close button 16 |
| `chrome/browser/ui/tabs/tab_style.{h,cc}` | `chromiumTabs.ts` (TabStyle metrics) | kTabWidth 232, top/bottom radii 10/12, separator 2×16, overlap 18, contents insets 20/12, pinned 64, min active 56, min inactive 32, declutter thresholds |
| `chrome/browser/ui/views/tabs/common/horizontal_tab_style_views.cc` | `chromiumTabs.ts` (`activeTabPath`, `topCornerRadiusForWidth`, squarcle constants) | GetPath()'s chrome-tab shape (extension arcs r12, top arcs r≤10), the detached-squarcle hover geometry |
| `chrome/browser/ui/color/tab_strip_color_mixer.cc` | `chromiumTabs.ts` (`STRIP_COLORS`) | active bg = toolbar, inactive bg = frame, hover = 40% blend, selected = 75%, control inks |
| `ui/color/ui_color_mixer.cc` | `chromiumTabs.ts` (`STRIP_COLORS`), `tokens.css` | kColorFrameActiveUnthemed #DEE1E6 / grey900, kColorToolbar #FFFFFF / #35363A |
| `components/tab_groups/tab_group_color.{h,cc}` | `chromiumTabs.ts` (`GROUP_COLOR_IDS`), `shell/tabs.rs` (`GROUP_COLORS = 9`) | the nine-color enum order — the wire format |
| `chrome/browser/ui/color/chrome_color_mixer.cc` | `chromiumTabs.ts` (`GROUP_TAB_STRIP_COLORS`), `tokens.css` | the classic palette values (light = kGoogle…600-class, dark = …300-class) |
| `chrome/browser/ui/tabs/tab_group_theme.cc` | index → color-id selection | SelectBasedOnDarkInput semantics |
| `chrome/browser/ui/views/tabs/common/tab_group_style.cc` + `common/tab_group_header_view.cc` | `chromiumTabs.ts` (chip constants), `frame.css` (`.group-chip`) | solid-color chip, radius 6, 20 DIP, top 4, max-contrast label |
| `chrome/browser/ui/views/tabs/tab_group_underline.{h,cc}` + `common/tab_group_line_view.cc` | `chromiumTabs.ts` (stroke law), `FrameApp.tsx` (boundary vars), `frame.css` (`.tab-group-underline`) | 2 DIP stroke, stroke inset 18, one-overlap above the floor, member/active boundary rules |
| `chrome/browser/ui/views/tabs/common/horizontal_tab_style_views.cc` (separator block) | `frame.css` (`.tab-separator`) | 2×16, radius 1, hanging from the band's padding line, 2.5-contrast color |

### The tab hover card (Task 14 wave)

| Upstream file | Ported into | What it governs |
| --- | --- | --- |
| `chrome/browser/ui/views/tabs/hovercard/tab_hover_card_bubble_view.{h,cc}` | `hoverCard.ts` (constants), `hoverCardView.tsx` + `hoverCard.css` (the card) | kHoverCardSlideDuration 200ms, title 2 lines, kTextMargins VH(12,12), kTitleDomainSpacing 4, width = GetPreviewImageSize().width() = 256, corner radius = GetCornerRadiusMetric(kHigh) = 8; GroupCardView: kGroupHovercardBorderMargins VH(6,12), kGroupTitleMargins VH(6,0) |
| `chrome/browser/ui/tabs/tab_group_data.h` + `tab_style.cc` | `hoverCard.ts` (`groupCardMembers`, `groupCardFooterText`, `groupCardHeader`) | kMaxTabs 5 member bullets, "+ N More" footer, "name (N tabs)" header (generated_resources.grd IDS_TAB_GROUPS_HOVER_CARD_HEADER, sentence case) |
| `chrome/browser/ui/color/chrome_color_mixer.cc` | `hoverCard.ts` (`HOVER_CARD_BACKGROUND`, icon foregrounds), `hoverCard.css` | card bg kGoogleGrey050 (dark kGoogleGrey900); secondary text = ui::kColorLabelForeground — same color as the title, the hierarchy is typography's |
| `chrome/browser/ui/views/tabs/hovercard/tab_hover_card_controller.cc` | `FrameApp.tsx` (the machine), `hoverCard.ts` (`hoverCardShowDelayMs`, `HOVER_CARD_RESHOW_BUFFER_MS`) | GetShowDelay's log law (300 floor, 800 log max, +500 at standard width, driven by the LARGEST tab in the strip), kShowWithoutDelayTimeBuffer 300ms, kEvent hide + PreventImmediateReshow, kAnimating hide, slide-between-tabs with no delay, kTabDataChanged refresh, FadeOut-then-close |
| `chrome/browser/ui/views/tabs/hovercard/fade_label_view.{h,cc}` | `hoverCardView.tsx` (`FadeLabel`) | the text crossfade: the previous text overlays and fades out over the new text, in the same commit |
| `ui/views/layout/layout_provider.cc` + `chrome/browser/ui/layout_constants.cc` | `hoverCard.ts` (radius, `HOVER_CARD_ANCHOR_GAP`) | Emphasis::kHigh radius 8; the card sits kTabStripPadding (6) below the slot |
| `chrome/browser/ui/views/tabs/hovercard/hover_card_anchor_target.cc` | `hoverCard.ts` (`hoverCardDomain`) | the domain law: the address without its scheme, hidden when empty (should_display_url=false posture) |

### The hover card's deliberate deltas

- **The widget carrier**: upstream paints the card in a bubble Widget
  floating over the content area, click-through to the page. The shell's
  equivalent widget is the popup overlay webview — created per show,
  sized to the payload's SLIDE BAND (the rect the card may roam:
  everything below the strip band horizontally, the rail column plus the
  card's reach vertically), never focused (SetCanActivate(false) +
  set_accept_events(false) upstream). Tauri's child webviews expose no
  click-through (set_ignore_cursor_events exists only on Window), so a
  press ON the band is the carrier's: the card yields it and closes
  itself, clearing the way for the next press — the one visible cost of
  the carrier, documented rather than silent. The browser demo paints
  the same component on a fixed full-page layer instead.
- **No thumbnails, alerts, memory, collaboration**: the TabCardView's
  preview/footer features need a capture pipeline and resource metrics
  the shell does not expose; the card is the ChromeOS-terminal InitParams
  posture (show_image_preview=false): title over domain. The group card
  is complete (header, five bullets, "+ N More").
- **Domain elision**: upstream elides the domain at the HEAD (ELIDE_HEAD
  keeps the tail); CSS offers only end ellipsis, so the line ellipsizes
  at the end.
- **Rail anchor clamp**: the anchor clamps against the frame webview's
  own width — in the rail posture that is the rail column, so the card
  may reach past the rail's right edge into the content (upstream clamps
  against the window; the frame cannot see the window's full width — the
  band's own reach margin widens the carrier to cover the card).
- **Wheel-hide**: scrolling the strip hides the card (the pointer's
  target moved with no boundary crossing); upstream re-evaluates on the
  next mousemove. The next crossing re-arms the delay either way.
- **Post-drag re-show**: upstream re-shows via MouseMoved after a drag
  ends; the card here re-arms on the next boundary crossing.
- **Instant destroy on menu-open**: a menu opening kills a showing card
  through dismiss_popup (no fade); upstream fades it. The click/keypress
  and leave hides DO fade (200ms) before the widget closes.

## Deliberate deltas (documented, not silent)

- **No C++/Views toolkit**: the port targets the webview — paths are SVG,
  paints are CSS custom properties. Geometry and color values are byte-for-
  byte upstream; the painting *mechanism* is idiomatic web.
- **Multi-selection** (`kDefaultSelectedTabOpacity`, selected-hover 85%):
  the constant is ported, but the strip has no multi-select yet, so no rule
  consumes it.
- **Split tabs, stacking**: upstream features the strip model does not
  expose yet; their constants are out of scope. (Hover cards were in this
  list once — they are ported now, see the Task 14 wave above.)
- **Close-button declutter** (hide at <100 DIP max width): needs the
  layout's max-width signal in the snapshot; pending until the model
  reports it.

## Tests

`chromiumTabs.test.ts` pins every derived law against the value in the
cited upstream file; `hoverCard.test.ts` does the same for the card (the
delay law's exact shape, the group card's strings, the anchor clamp). When
re-porting a newer Chromium, run them first: a red pin means upstream
changed — read the cited file, then re-derive.

## Task 16 — the tab multi-selection (the selected fill's consumer)

Files ported (chromium/src main, fetched 2026-10-10):

- `ui/base/models/list_selection_model.{h,cc}` — the selection state's
  shape: selected set + active + anchor, and the mutation verbs
  (AddIndexToSelection, SetSelectionFromAnchorTo, AddSelectionFromAnchorTo,
  IncrementFrom/DecrementFrom for insert/remove). ZIM stores the set by
  STABLE TAB ID, so the index shuffles are structurally unneeded.
- `chrome/browser/ui/views/tabs/tab.cc` — `Tab::OnMousePressed`'s gesture
  dispatch, verbatim in behavior: on the PRESS, shift+ctrl →
  AddSelectionFromAnchorTo; shift → ExtendSelectionTo; ctrl →
  ToggleSelected (a deselecting ctrl-press refuses to arm a drag —
  "don't allow dragging non-selected tabs"); plain → SelectTab only when
  the tab is not already selected.
- `chrome/browser/ui/views/tabs/browser_tab_strip_controller.cc` —
  ToggleSelected = selected ? DeselectTabAt : SelectTabAt.
- `chrome/browser/ui/tabs/tab_strip_model.cc` — SelectTabAt (add +
  anchor), DeselectTabAt ("one tab must be selected"; the promotion of
  the FIRST SELECTED when the active or anchor leaves), ExtendSelectionTo
  (the anchor range REPLACES the selection, active = clicked, anchor
  stays), AddSelectionFromAnchorTo (the range ADDS, active = clicked),
  GetIndicesForCommand (a SELECTED context tab commands the whole
  selection — the scope law behind the menu's close), and the close-side
  removal maintenance.
- `chrome/browser/ui/tabs/tab_menu_model.cc` — the close item's plural
  (IDS_TAB_CXMENU_CLOSETAB, "Close {N,plural, =1 {tab} other {# tabs}}").
- `chrome/browser/ui/color/paints` — the selected fill:
  `tab_strip_color_mixer.cc`'s selected law, kDefaultSelectedTabOpacity
  = 0.75 of the toolbar over the frame → `--strip-selected: #f7f7f7`,
  OPAQUE (the hover's overlap law applies).

### Documented deltas

- **ctrl-add does not activate.** Current main's `SelectTabAt` also calls
  `SetActiveTab(clicked)` (tab_strip_model.cc:1567) — the split refactor's
  detail. The model's own unittests treat Select and Activate as distinct
  verbs and pin no activation in the selection tests; the classic
  non-activating toggle stands. FLAGGED for the Windows eyeball pass:
  compare ctrl+click against installed Chrome and flip the one branch if
  upstream truly activates.
- **Split tabs** do not exist in ZIM — every split branch
  (AppendTabsToSelection of a split's members, split-aware ranges) is
  structurally absent; `focused_group` (group-focus mode) is likewise
  out of scope.
- **Dragging a multi-selection** is not wired yet: upstream arms the
  drag with the whole selection (`MaybeStartDrag(this, event,
  original_selection)`) and moves it as a block (MoveSelectedTabsTo).
  ZIM's modifier presses complete the gesture but never arm a drag; the
  model's `move_to`/`reorder_drop` still move one tab. The next port.
- **Selected-hover** renders the same 75% fill (upstream mixes
  selected-hover at 85% — kHoveredSelectedTabOpacity — unused until the
  two-opacity law has a consumer that needs the distinction).
