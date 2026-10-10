// The tab hover card's law — the port of Chromium's hover card
// (chrome/browser/ui/views/tabs/hovercard/). Every constant cites its
// upstream home; the tests pin them so a re-port cannot drift. The
// geometry constants ride the already-ported tab strip law
// (chromiumTabs.ts) — the card is sized BY the strip's own widths.
// See PROVENANCE.md for the file ledger and the documented deltas.

import {
  STANDARD_SLOT_WIDTH,
  PINNED_WIDTH,
  TAB_STRIP_PADDING,
} from "./chromiumTabs";

// tab_hover_card_bubble_view.h: the one duration every card transition
// rides — the show fade, the between-tab slide, the text crossfade.
export const HOVER_CARD_SLIDE_DURATION_MS = 200;

// ui/views/layout/layout_provider.cc GetCornerRadiusMetric(Emphasis::kHigh)
// — the bubble's corner_radius_ (tab_hover_card_bubble_view.h).
export const HOVER_CARD_CORNER_RADIUS = 8;

// tab_style.cc GetPreviewImageSize: the card's width IS the standard slot
// width (the preview image's width law). The 16:9 box that law also
// defines is the thumbnail ZIM does not paint — no capture pipeline, the
// ChromeOS-terminal InitParams posture (show_image_preview=false).
export const HOVER_CARD_WIDTH = STANDARD_SLOT_WIDTH; // 256

// tab_hover_card_bubble_view.cc anonymous namespace:
export const HOVER_CARD_TITLE_MAX_LINES = 2;
/** kTextMargins — the margins around the title/domain text. */
export const HOVER_CARD_TEXT_MARGINS = { v: 12, h: 12 };
/** kTitleDomainSpacing — the gap between title and domain lines. */
export const HOVER_CARD_TITLE_DOMAIN_SPACING = 4;

// Group card (tab_hover_card_bubble_view.cc GroupCardView + tab_group_data.h):
/** kGroupHovercardBorderMargins — the group card's own border. */
export const GROUP_CARD_BORDER_MARGINS = { v: 6, h: 12 };
/** kGroupTitleMargins — the margins around each group card line. */
export const GROUP_CARD_ITEM_MARGINS = { v: 6, h: 0 };
/** tabs::TabGroupData::kMaxTabs — member lines before the footer counts. */
export const GROUP_CARD_MAX_TABS = 5;

// generated_resources.grd: IDS_LIST_BULLET is the bullet, two spaces,
// the text; IDS_TAB_GROUPS_HOVER_CARD_FOOTER is "+ N More".
export const GROUP_CARD_BULLET = "\u2022  ";
export const groupCardFooterText = (excess: number): string =>
  `+ ${excess} More`;

// The group card's header law (IDS_TAB_GROUPS_HOVER_CARD_HEADER, the
// sentence-case variant, plus the unnamed variant): "name (1 tab)" /
// "name (N tabs)"; an unnamed group's header is just the count.
export const groupCardHeader = (
  label: string | null,
  tabCount: number,
): string => {
  const count = tabCount === 1 ? "1 tab" : `${tabCount} tabs`;
  return label != null && label !== "" ? `${label} (${count})` : count;
};

// GroupCardData's composition law: at most kMaxTabs member lines; the
// rest are counted into the "+ N More" footer.
export const groupCardMembers = (
  titles: string[],
): { members: string[]; excess: number } => ({
  members: titles.slice(0, GROUP_CARD_MAX_TABS),
  excess: Math.max(0, titles.length - GROUP_CARD_MAX_TABS),
});

// chrome_color_mixer.cc: the card's surfaces. The light pair is the
// panel's own posture; the dark pair is recorded so a dark port stays
// honest. The icon foreground colors the placeholder/crashed preview
// icons ZIM does not paint (no preview feature) — kept for the ledger.
export const HOVER_CARD_BACKGROUND = "#f8f9fa"; // kGoogleGrey050 (dark: kGoogleGrey900 #202124)
export const HOVER_CARD_ICON_FOREGROUND_LIGHT = "#dadce0"; // kGoogleGrey300
export const HOVER_CARD_ICON_FOREGROUND_DARK = "#5f6368"; // kGoogleGrey700
// kColorTabHoverCardSecondaryText resolves to ui::kColorLabelForeground —
// the SAME color the title paints; the hierarchy is typography's (13px
// medium over 12px regular), not the palette's.

// tab_hover_card_controller.cc GetShowDelay — the delay shrinks with the
// strip's promise of legibility. It is computed from the LARGEST tab in
// the strip (so every tab of a strip answers the same way), runs 300→800
// on a log scale between the pinned and standard widths, and adds 500ms
// more once a tab reaches standard width — the card adds least where the
// tab itself already shows everything.
export const HOVER_CARD_MIN_SHOW_DELAY_MS = 300; // kMinimumTriggerDelay
export const HOVER_CARD_MAX_SHOW_DELAY_MS = 800; // kMaximumTriggerDelay
export const HOVER_CARD_MAX_WIDTH_EXTRA_DELAY_MS = 500; // kTabHoverCardAdditionalMaxWidthDelay default
export const hoverCardShowDelayMs = (
  tabWidth: number,
  pinnedWidth: number = PINNED_WIDTH,
  standardWidth: number = STANDARD_SLOT_WIDTH,
): number => {
  if (tabWidth <= pinnedWidth) return HOVER_CARD_MIN_SHOW_DELAY_MS;
  const denominator = Math.log(standardWidth - pinnedWidth + 1);
  if (!(denominator > 0)) {
    // Degenerate strip (standard ≤ pinned with a somehow-wider tab): the
    // log cannot interpolate (negative/NaN) — ride the max.
    return HOVER_CARD_MAX_SHOW_DELAY_MS + HOVER_CARD_MAX_WIDTH_EXTRA_DELAY_MS;
  }
  const fraction = Math.log(tabWidth - pinnedWidth + 1) / denominator;
  let delay =
    fraction * (HOVER_CARD_MAX_SHOW_DELAY_MS - HOVER_CARD_MIN_SHOW_DELAY_MS) +
    HOVER_CARD_MIN_SHOW_DELAY_MS;
  if (tabWidth >= standardWidth) delay += HOVER_CARD_MAX_WIDTH_EXTRA_DELAY_MS;
  return delay;
};

// tab_hover_card_controller.cc ShouldShowImmediately — a re-entry within
// this buffer of the last exit shows the card without waiting (the
// pointer grazing the strip's edge must not re-arm the whole delay).
export const HOVER_CARD_RESHOW_BUFFER_MS = 300; // kShowWithoutDelayTimeBuffer

// The card's gap below the hovered slot: kTabStripPadding
// (layout_constants.cc), the same 6 that forms the strip band.
export const HOVER_CARD_ANCHOR_GAP = TAB_STRIP_PADDING; // 6

// The anchor law: centered on the slot, clamped inside the window's
// width (the bubble's PreferredArrowAdjustment::kOffset does this
// upstream), 6 below the slot's bottom edge.
export const hoverCardAnchor = (
  slotRect: { left: number; width: number; bottom: number },
  clampWidth: number,
): { x: number; y: number } => {
  const x = Math.min(
    Math.max(slotRect.left + slotRect.width / 2 - HOVER_CARD_WIDTH / 2, 8),
    Math.max(clampWidth - HOVER_CARD_WIDTH - 8, 8),
  );
  return { x, y: slotRect.bottom + HOVER_CARD_ANCHOR_GAP };
};

// The domain law (hover_card_anchor_target.cc SetHoverCardDataFrom +
// url_formatter::FormatUrl's omit-scheme posture): the address without
// its scheme. ZIM's pages are zim:// URLs, so the card's "domain" is the
// rest of the address — "server/survival", "join/localhost:25565",
// "settings/". An empty remainder hides the label, exactly upstream's
// should_display_url=false posture.
export const hoverCardDomain = (
  url: string | null | undefined,
): string | null => {
  if (url == null) return null;
  const scheme = "zim://";
  const rest = url.startsWith(scheme) ? url.slice(scheme.length) : url;
  const trimmed = rest.trim();
  return trimmed === "" ? null : trimmed;
};

// GetShowDelay's consistency law: the LARGEST tab in the strip drives
// the delay. Header slots are chips, not tabs.
export const largestTabSlotWidth = (
  slots: Array<{ header?: boolean; width: number }>,
): number =>
  slots.reduce((max, s) => (s.header ? max : Math.max(max, s.width)), 0);

// The slide band's law: the window rectangle the carrier webview covers
// while the card lives. UPSTREAM the bubble is an input-transparent
// Widget (SetCanActivate(false) — presses pass through); a webview
// carrier has no click-through, so the band must NEVER be bigger than
// the card's own corridor: the union of the current and previous anchors
// (the slide roams exactly that far) plus a small roam margin, and the
// card's own height plus the bottom margin. The first law ("everything
// below the strip") covered the toolbar and the whole content area — a
// showing card ate the operator's first click anywhere under the strip.
// The carrier is re-bounded on every slide (shell_popup_update), and a
// press ON the card yields it (PopupApp's law — the documented delta).
export const HOVER_CARD_BAND_ROAM_MARGIN = 24; // the slide's breathing room per side
export const HOVER_CARD_BAND_BOTTOM_MARGIN = 12; // the shadow's reach below the card
/** The card's painted height: the tab card is title (≤2 lines) + domain;
 *  the group card adds up to five member lines and a footer. The law is
 *  per kind because the band's height is the card's own reach. */
export const hoverCardHeightFor = (kind: "tab" | "group"): number =>
  kind === "tab"
    ? HOVER_CARD_TEXT_MARGINS.v * 2 +
      HOVER_CARD_TITLE_MAX_LINES * 18 +
      HOVER_CARD_TITLE_DOMAIN_SPACING +
      16
    : GROUP_CARD_BORDER_MARGINS.v * 2 +
      18 +
      GROUP_CARD_MAX_TABS * (18 + GROUP_CARD_ITEM_MARGINS.v) +
      18 +
      HOVER_CARD_TITLE_DOMAIN_SPACING;
export const hoverCardBand = (
  kind: "tab" | "group",
  current: { x: number; y: number },
  previous: { x: number; y: number } | null,
  viewport: { w: number; h: number },
): { x: number; y: number; w: number; h: number } => {
  const height = hoverCardHeightFor(kind);
  const left =
    Math.min(current.x, previous?.x ?? current.x) - HOVER_CARD_BAND_ROAM_MARGIN;
  const right =
    Math.max(
      current.x + HOVER_CARD_WIDTH,
      (previous?.x ?? current.x) + HOVER_CARD_WIDTH,
    ) + HOVER_CARD_BAND_ROAM_MARGIN;
  return {
    x: Math.max(0, left),
    y: Math.max(0, current.y),
    w: Math.min(viewport.w, right) - Math.max(0, left),
    h: Math.min(
      Math.max(viewport.h - current.y, 1),
      height + HOVER_CARD_BAND_BOTTOM_MARGIN,
    ),
  };
};

// The card's display payload: the frame dresses it (the laws run there —
// one home, no twin to drift), the overlay only paints it.
export interface HoverCardPayload {
  kind: "tab" | "group";
  /** The card's top-left corner in WINDOW coordinates. */
  x: number;
  y: number;
  /** The slide band: the carrier's window rectangle — the union of the
   *  current and previous anchors plus the roam margin, as tall as the
   *  card itself (hoverCardBand's law). The host re-bounds the carrier
   *  to this rect on every slide; the rest of the window keeps its own
   *  pointer (upstream's bubble is input-transparent; the carrier is
   *  not, so the band stays the card's corridor). */
  band: { x: number; y: number; w: number; h: number };
  /** Tab cards: the tab's title. Group cards: the composed header line. */
  title: string;
  /** Tab cards: the scheme-less address, or null to hide the line. */
  domain: string | null;
  /** Group cards: at most kMaxTabs member titles, model order. */
  members: string[];
  /** Group cards: the count beyond the shown members (0 → no footer). */
  excess: number;
}
