// Chromium's tab strip law, backported. EVERY constant, formula, and color
// here is copied from Chromium's own sources — nothing is invented. The
// upstream file and revision are cited per block; when upstream changes,
// re-read that file and re-port.
//
// Upstream revision: chromium/src main @ 157.0.8097.0 (fetched 2026-10-10).
// License: BSD-style, Copyright The Chromium Authors (see LICENSE.CHROMIUM
// next to this file). This module is the single source of truth for the
// frame's strip visuals; frame.css and FrameApp.tsx only consume it.
//
// Ported blocks (upstream path → here):
//   chrome/browser/ui/layout_constants.{h,cc}      → Layout constants
//   chrome/browser/ui/tabs/tab_style.{h,cc}        → TabStyle metrics
//   chrome/browser/ui/views/tabs/common/horizontal_tab_style_views.cc
//                                                  → GetPath(), corner law
//   chrome/browser/ui/color/tab_strip_color_mixer.cc
//     + ui/color/ui_color_mixer.cc                 → the color pipeline
//   components/tab_groups/tab_group_color.{h,cc}   → the group color enum
//   chrome/browser/ui/color/chrome_color_mixer.cc  → group palette values
//   chrome/browser/ui/tabs/tab_group_theme.cc      → id → color-id mapping
//   chrome/browser/ui/views/tabs/common/tab_group_style.cc
//                                                  → header chip metrics
//   chrome/browser/ui/views/tabs/tab_group_underline.{h,cc}
//     + common/tab_group_line_view.cc              → the group underline

// -- Layout constants (layout_constants.cc) --------------------------------
// kTabHeight = 34 + kTabstripToolbarOverlap; kTabStripHeight = kTabHeight +
// kTabStripPadding. The band the strip lives in is 35 + 6 = 41 DIP.
export const TAB_HEIGHT = 35;
export const TABSTRIP_TOOLBAR_OVERLAP = 1;
export const TAB_STRIP_PADDING = 6;
export const TAB_STRIP_HEIGHT = TAB_HEIGHT + TAB_STRIP_PADDING; // 41
export const TAB_HORIZONTAL_PADDING = 8;
export const TAB_VERTICAL_PADDING = 6;
export const TAB_CLOSE_BUTTON_SIZE = 16; // non-touch, non-rounded-icons
export const TAB_PRE_TITLE_PADDING = 8;
export const TAB_SEPARATOR_HEIGHT = 20; // kTabSeparatorHeight, non-touch
export const FAVICON_SIZE = 16; // gfx::kFaviconSize

// -- TabStyle metrics (tab_style.cc) ----------------------------------------
// The standard tab width is 232 DIP, excluding separators and overlap. A
// tab's SLOT span adds the two bottom-corner extensions (its path paints
// 12 DIP into each neighbor's overlap zone).
const TAB_WIDTH = 232;
export const TOP_CORNER_RADIUS = 10;
export const BOTTOM_CORNER_RADIUS = 12;
const SEPARATOR_THICKNESS = 2;
const SEPARATOR_HORIZONTAL_MARGIN = 2;
const SEPARATOR_HEIGHT = 16;
const INTERIOR_WIDTH = 16; // kInteriorWidth: min-tab appearance law
const TAB_PINNED_CONTENT_WIDTH = 24;
export const TAB_STRIP_DECLUTTER_MAX_TAB_WIDTH_FOR_CLOSE_HIDE = 100;
export const TAB_STRIP_DECLUTTER_MIN_TABS_FOR_SEPARATOR_HIDE = 20;

// GetStandardWidth(false): kTabWidth + 2 * GetBottomCornerRadius().
export const STANDARD_SLOT_WIDTH = TAB_WIDTH + 2 * BOTTOM_CORNER_RADIUS; // 256
// GetTabOverlap(): 2 * bottom radius − separator width − its margins.
const SEPARATOR_TOTAL_WIDTH = SEPARATOR_THICKNESS + 2 * SEPARATOR_HORIZONTAL_MARGIN;
export const TAB_OVERLAP = 2 * BOTTOM_CORNER_RADIUS - SEPARATOR_TOTAL_WIDTH; // 18
// GetContentsInsets(): bottom radius + horizontal padding, each side.
export const CONTENTS_INSET_X = BOTTOM_CORNER_RADIUS + TAB_HORIZONTAL_PADDING; // 20
export const CONTENTS_INSET_Y = TAB_VERTICAL_PADDING + TAB_STRIP_PADDING; // 12
// GetPinnedWidth(false): content + both contents insets.
export const PINNED_WIDTH = TAB_PINNED_CONTENT_WIDTH + 2 * CONTENTS_INSET_X; // 64
// GetMinimumActiveWidth(false): close button (>= favicon) + insets.
export const MIN_ACTIVE_WIDTH = TAB_CLOSE_BUTTON_SIZE + 2 * CONTENTS_INSET_X; // 56
// GetMinimumInactiveWidth(): interior − separator + overlap.
export const MIN_INACTIVE_WIDTH = INTERIOR_WIDTH - SEPARATOR_THICKNESS + TAB_OVERLAP; // 32
// Selected-tab opacity over inactive multi-selected tabs (not used yet —
// the strip has no multi-selection; kept for the port's completeness).
export const SELECTED_TAB_OPACITY = 0.75; // kDefaultSelectedTabOpacity

// HorizontalTabStyleViews::GetTopCornerRadiusForWidth — the top of a narrow
// tab keeps at least a third of its width flat before the corners start.
export const topCornerRadiusForWidth = (width: number): number => {
  const topWidth = width - TOP_CORNER_RADIUS * 2;
  return Math.min(Math.max(topWidth / 3, 0), TOP_CORNER_RADIUS);
};

// -- GetPath(): the active tab's fill (horizontal_tab_style_views.cc) -------
// The path runs clockwise from the lower-left, in the 35-DIP tab view:
// left extension edge → bottom-left arc (r 12, CCW) → ascender → top-left
// arc (r top, CW) → crossbar → top-right arc → descender → bottom-right arc
// → right extension edge, closed along the bottom. The top radius follows
// GetTopCornerRadiusForWidth; the bottom extensions use the full 12.
export const activeTabPath = (width: number): string => {
  const rTop = topCornerRadiusForWidth(width);
  const rExt = BOTTOM_CORNER_RADIUS;
  const tabLeft = rExt;
  const tabRight = width - rExt;
  const tabBottom = TAB_HEIGHT - TABSTRIP_TOOLBAR_OVERLAP; // 34
  const f = (n: number): string => (Number.isInteger(n) ? `${n}` : n.toFixed(2));
  return [
    `M 0 ${TAB_HEIGHT}`,
    `L 0 ${tabBottom}`,
    `L ${f(tabLeft - rExt)} ${tabBottom}`,
    `A ${rExt} ${rExt} 0 0 0 ${f(tabLeft)} ${f(tabBottom - rExt)}`,
    `L ${f(tabLeft)} ${f(rTop)}`,
    `A ${rTop} ${rTop} 0 0 1 ${f(tabLeft + rTop)} 0`,
    `L ${f(tabRight - rTop)} 0`,
    `A ${rTop} ${rTop} 0 0 1 ${f(tabRight)} ${f(rTop)}`,
    `L ${f(tabRight)} ${f(tabBottom - rExt)}`,
    `A ${rExt} ${rExt} 0 0 0 ${f(width)} ${f(tabBottom)}`,
    `L ${f(width)} ${TAB_HEIGHT}`,
    "Z",
  ].join(" ");
};

// The inactive/hover fill is the "detached squarcle" (kHighlight path): a
// rounded rect inset one bottom-radius from each slot edge, one strip
// padding down from the band top, kTabHeight − padding − overlap tall.
// Narrow tabs (content under a favicon wide) expand into the separators'
// zone — half the separator's total width per side.
export const SQUARCLE_INSET_X = BOTTOM_CORNER_RADIUS; // 12
export const SQUARCLE_HEIGHT = TAB_HEIGHT - TAB_STRIP_PADDING - TABSTRIP_TOOLBAR_OVERLAP; // 28
export const SQUARCLE_TIGHT_INSET_X = BOTTOM_CORNER_RADIUS - SEPARATOR_TOTAL_WIDTH / 2; // 9

// -- Color math (ui/gfx/color_utils.h semantics) -----------------------------
/** AlphaBlend(fg, bg, alpha): `fg` composited at `alpha` over `bg`. */
export function alphaBlend(fg: string, bg: string, alpha: number): string {
  const a = hex(fg);
  const b = hex(bg);
  const mix = (x: number, y: number): number => Math.round(x * alpha + y * (1 - alpha));
  const r = mix(a[0], b[0]);
  const g = mix(a[1], b[1]);
  const bl = mix(a[2], b[2]);
  return `#${[r, g, bl].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/** WCAG contrast ratio (color_utils::ContrastRatio). */
export function contrastRatio(fg: string, bg: string): number {
  const l1 = luminance(fg);
  const l2 = luminance(bg);
  return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
}

function luminance(hexColor: string): number {
  const [r, g, b] = hex(hexColor);
  const channel = (c: number): number => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function hex(color: string): [number, number, number] {
  const h = color.replace("#", "");
  return [
    parseInt(h.slice(0, 2), 16),
    parseInt(h.slice(2, 4), 16),
    parseInt(h.slice(4, 6), 16),
  ];
}

// -- The color pipeline (default, non-themed values) ------------------------
// tab_strip_color_mixer.cc: active tab bg = kColorToolbar; inactive bg =
// kColorFrameActive; hover = 40% of the active bg over the inactive bg;
// selected = 75%. ui_color_mixer.cc: kColorFrameActiveUnthemed = #DEE1E6
// (light) / kGoogleGrey900 (dark); kColorToolbar = white / #35363A.
export interface StripColors {
  frame: string; // the strip's ground (kColorFrameActiveUnthemed)
  toolbar: string; // the active tab's body (kColorToolbar)
  tabFg: string; // kColorToolbarText
  inactiveHover: string; // kColorTabBackgroundInactiveHoverFrameActive
  controlInk: string; // kColorTabStripControlButtonInkDrop (16% ink)
}
export const STRIP_COLORS: Record<"light" | "dark", StripColors> = {
  light: {
    frame: "#dee1e6",
    toolbar: "#ffffff",
    tabFg: "#3c4043", // kGoogleGrey800
    inactiveHover: alphaBlend("#ffffff", "#dee1e6", 0.4),
    controlInk: "rgba(32, 33, 36, 0.16)",
  },
  dark: {
    frame: "#202124", // gfx::kGoogleGrey900
    toolbar: "#35363a",
    tabFg: "#ffffff",
    inactiveHover: alphaBlend("#35363a", "#202124", 0.4),
    controlInk: "rgba(255, 255, 255, 0.16)",
  },
};

// -- Group colors (components/tab_groups + chrome_color_mixer.cc) -----------
// The enum order IS the wire format (tab_group_color.h: values are written
// to disk — do not reorder). The values are the CLASSIC palette entries the
// mixer assigns when the color-refresh feature is off: SelectBasedOnDarkInput
// picks base_dark (the kGoogle…300-class) in dark mode, base_light (the deep
// kGoogle…600-class) in light mode. The frame is the light theme today; the
// dark values ride along so the day the band goes dark the law is already
// here.
export interface GroupColor {
  id: string; // TabGroupColorId name
  label: string; // GetTabGroupColorLabelMap()'s label
  light: string; // base_light — the tab strip color in light mode
  dark: string; // base_dark — the tab strip color in dark mode
}
export const GROUP_COLOR_IDS = [
  "grey",
  "blue",
  "red",
  "yellow",
  "green",
  "pink",
  "purple",
  "cyan",
  "orange",
] as const;
export type GroupColorId = (typeof GROUP_COLOR_IDS)[number];
export const GROUP_TAB_STRIP_COLORS: readonly GroupColor[] = [
  { id: "grey", label: "Grey", light: "#5f6368", dark: "#dadce0" }, // kGoogleGrey700 / 300
  { id: "blue", label: "Blue", light: "#1a73e8", dark: "#8ab4f8" }, // kGoogleBlue600 / 300
  { id: "red", label: "Red", light: "#d93025", dark: "#f28b82" }, // kGoogleRed600 / 300
  { id: "yellow", label: "Yellow", light: "#f9ab00", dark: "#fdd663" }, // kGoogleYellow600 / 300
  { id: "green", label: "Green", light: "#188038", dark: "#81c995" }, // kGoogleGreen700 / 300
  { id: "pink", label: "Pink", light: "#d01884", dark: "#ff8bcb" }, // kGooglePink700 / 300
  { id: "purple", label: "Purple", light: "#a142f4", dark: "#c58af9" }, // kGooglePurple500 / 300
  { id: "cyan", label: "Cyan", light: "#007b83", dark: "#78d9ec" }, // kGoogleCyan900 / 300
  { id: "orange", label: "Orange", light: "#fa903e", dark: "#fcad70" }, // kGoogleOrange400 / 300
];
export const groupColorCount = GROUP_TAB_STRIP_COLORS.length; // kNumEntries = 9
export const groupTabStripColor = (index: number): GroupColor => {
  const at = ((index % groupColorCount) + groupColorCount) % groupColorCount;
  const color = GROUP_TAB_STRIP_COLORS.at(at);
  if (!color) throw new Error(`group color index ${index} escaped the enum`);
  return color;
};

// -- Group header chip (tab_group_style.cc + tab_group_header_view.cc) ------
export const GROUP_CHIP_CORNER_RADIUS = 6; // kCornerRadius (horizontal)
export const GROUP_CHIP_VERTICAL_INSET = 2; // kHeaderChipVerticalInset
export const GROUP_CHIP_EMPTY_SIZE = 20; // kEmptyChipSize
export const GROUP_TAB_GROUP_OVERLAP_ADJUSTMENT = 2; // kTabGroupOverlapAdjustment
export const GROUP_LEADING_HEADER_PADDING = TAB_OVERLAP - GROUP_TAB_GROUP_OVERLAP_ADJUSTMENT; // 16
export const GROUP_COLLAPSED_HEADER_PADDING =
  TAB_OVERLAP - 2 * GROUP_TAB_GROUP_OVERLAP_ADJUSTMENT; // 14
// The chip rides (kTabStripHeight − empty chip − overlap) / 2 from the band's
// top (GetTitleChipOffset's y: 10 DIP); inside the 35-DIP tab view — which
// starts one strip padding down — that is 4.
export const GROUP_CHIP_TOP =
  (TAB_STRIP_HEIGHT - GROUP_CHIP_EMPTY_SIZE - TABSTRIP_TOOLBAR_OVERLAP) / 2 -
  TAB_STRIP_PADDING; // 4
// The chip's body is the group's SOLID tab strip color; its label is the
// max-contrast color over that body (tab_group_header_view.cc's
// GetForegroundColor → GetColorWithMaxContrast).
export const groupChipForeground = (bg: string): "#ffffff" | "#000000" =>
  contrastRatio(bg, "#ffffff") >= contrastRatio(bg, "#000000") ? "#ffffff" : "#000000";

// -- The group underline (tab_group_underline.{h,cc}) ------------------------
export const GROUP_LINE_STROKE_THICKNESS = 2; // FocusRing::kDefaultHaloThickness
// GetStrokeInset(): overlap − overlap adjustment + stroke thickness.
export const GROUP_LINE_STROKE_INSET =
  TAB_OVERLAP - GROUP_TAB_GROUP_OVERLAP_ADJUSTMENT + GROUP_LINE_STROKE_THICKNESS; // 18
// The horizontal line's caps round at half the stroke (tab_group_line_view.cc
// rounds the VERTICAL line at 4 — kGroupLineCornerRadius — and paints the
// horizontal one as a stroke).
export const GROUP_LINE_CORNER_RADIUS = 1;
// The line rides the strip's floor: y = bottom − toolbar overlap − stroke
// (tab_group_underline.cc's CalculateTabGroupUnderlineBounds).
export const GROUP_LINE_BOTTOM = TABSTRIP_TOOLBAR_OVERLAP; // 1

// -- Separators (tab_style.cc) ----------------------------------------------
export const SEPARATOR_SIZE = { width: SEPARATOR_THICKNESS, height: SEPARATOR_HEIGHT };
export const SEPARATOR_CORNER_RADIUS = SEPARATOR_THICKNESS / 2; // 1
// GetContrastRatioValues(): the separator color is the foreground blended
// over the inactive background until it reads at 2.5 contrast
// (kTabSeparatorContrast). BlendForMinContrast finds the smallest such
// alpha — a binary search reproduces it.
export const separatorColor = (bg: string, fg: string): string => {
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 24; i++) {
    const mid = (lo + hi) / 2;
    if (contrastRatio(alphaBlend(fg, bg, mid), bg) >= 2.5) hi = mid;
    else lo = mid;
  }
  return alphaBlend(fg, bg, hi);
};
