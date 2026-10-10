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

## Deliberate deltas (documented, not silent)

- **No C++/Views toolkit**: the port targets the webview — paths are SVG,
  paints are CSS custom properties. Geometry and color values are byte-for-
  byte upstream; the painting *mechanism* is idiomatic web.
- **Multi-selection** (`kDefaultSelectedTabOpacity`, selected-hover 85%):
  the constant is ported, but the strip has no multi-select yet, so no rule
  consumes it.
- **Split tabs, stacking, hover cards**: upstream features the strip model
  does not expose yet; their constants are out of scope.
- **Close-button declutter** (hide at <100 DIP max width): needs the
  layout's max-width signal in the snapshot; pending until the model
  reports it.

## Tests

`chromiumTabs.test.ts` pins every derived law against the value in the
cited upstream file. When re-porting a newer Chromium, run it first: a red
pin means upstream changed — read the cited file, then re-derive.
