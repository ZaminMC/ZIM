// The hover card law's pins. The delay law is upstream's logarithm —
// these tests assert its exact shape so a re-port cannot quietly become
// a flat timer; the group card's composition is upstream's string law.

import { describe, expect, it } from "vitest";
import {
  GROUP_CARD_MAX_TABS,
  HOVER_CARD_ANCHOR_GAP,
  HOVER_CARD_RAIL_REACH_MARGIN,
  HOVER_CARD_CORNER_RADIUS,
  HOVER_CARD_MAX_SHOW_DELAY_MS,
  HOVER_CARD_MAX_WIDTH_EXTRA_DELAY_MS,
  HOVER_CARD_MIN_SHOW_DELAY_MS,
  HOVER_CARD_RESHOW_BUFFER_MS,
  HOVER_CARD_SLIDE_DURATION_MS,
  HOVER_CARD_WIDTH,
  groupCardFooterText,
  groupCardHeader,
  groupCardMembers,
  hoverCardAnchor,
  hoverCardBand,
  hoverCardDomain,
  hoverCardShowDelayMs,
  largestTabSlotWidth,
} from "./hoverCard";
import { PINNED_WIDTH, STANDARD_SLOT_WIDTH } from "./chromiumTabs";

describe("the hover card's geometry", () => {
  it("is the standard slot width wide — the preview image's width law", () => {
    // tab_style.cc GetPreviewImageSize: width = GetStandardWidth(false)
    // = kTabWidth + two bottom-corner extensions = 232 + 2×12.
    expect(HOVER_CARD_WIDTH).toBe(256);
    expect(HOVER_CARD_WIDTH).toBe(STANDARD_SLOT_WIDTH);
  });

  it("wears the high-emphasis corner radius (8) and the 200ms slide", () => {
    // layout_provider.cc GetCornerRadiusMetric(Emphasis::kHigh);
    // tab_hover_card_bubble_view.h kHoverCardSlideDuration.
    expect(HOVER_CARD_CORNER_RADIUS).toBe(8);
    expect(HOVER_CARD_SLIDE_DURATION_MS).toBe(200);
  });

  it("anchors centered, 6 below the slot, clamped inside the window", () => {
    // kTabStripPadding below; the bubble's kOffset arrow adjustment
    // clamps; 8 is the popup_rect clamp's own margin (the host's law).
    const anchor = hoverCardAnchor({ left: 300, width: 40, bottom: 41 }, 1024);
    expect(anchor.y).toBe(41 + HOVER_CARD_ANCHOR_GAP);
    expect(anchor.x).toBe(300 + 20 - HOVER_CARD_WIDTH / 2); // centered

    // Off the left edge: the clamp holds 8.
    const clampedLeft = hoverCardAnchor({ left: 0, width: 10, bottom: 41 }, 1024);
    expect(clampedLeft.x).toBe(8);

    // Off the right edge: the card's right side holds 8.
    const clampedRight = hoverCardAnchor({ left: 1010, width: 40, bottom: 41 }, 1024);
    expect(clampedRight.x).toBe(1024 - HOVER_CARD_WIDTH - 8);

    // A window narrower than the card: the left margin wins (no
    // negative x — the card never leaves the window).
    const narrow = hoverCardAnchor({ left: 50, width: 40, bottom: 41 }, 200);
    expect(narrow.x).toBe(8);
  });
});

describe("the show delay law (tab_hover_card_controller.cc GetShowDelay)", () => {
  it("is the floor at pinned width — a pinned tab gives its title up immediately", () => {
    expect(hoverCardShowDelayMs(PINNED_WIDTH)).toBe(HOVER_CARD_MIN_SHOW_DELAY_MS);
    expect(hoverCardShowDelayMs(10)).toBe(HOVER_CARD_MIN_SHOW_DELAY_MS);
  });

  it("adds the full-width extra at standard width — 300 + 500 + the log term", () => {
    // At standard width the log fraction is exactly 1: 300 + (800−300) = 800,
    // then kTabHoverCardAdditionalMaxWidthDelay's 500 more.
    expect(hoverCardShowDelayMs(STANDARD_SLOT_WIDTH)).toBe(
      HOVER_CARD_MIN_SHOW_DELAY_MS +
        (HOVER_CARD_MAX_SHOW_DELAY_MS - HOVER_CARD_MIN_SHOW_DELAY_MS) +
        HOVER_CARD_MAX_WIDTH_EXTRA_DELAY_MS,
    );
  });

  it("interpolates monotonically between the pinned and standard widths", () => {
    let previous = HOVER_CARD_MIN_SHOW_DELAY_MS;
    for (let width = PINNED_WIDTH + 1; width < STANDARD_SLOT_WIDTH; width += 8) {
      const delay = hoverCardShowDelayMs(width);
      expect(delay).toBeGreaterThan(previous);
      expect(delay).toBeLessThan(
        HOVER_CARD_MAX_SHOW_DELAY_MS + HOVER_CARD_MAX_WIDTH_EXTRA_DELAY_MS,
      );
      previous = delay;
    }
  });

  it("keeps the floor first on a degenerate strip — upstream's order", () => {
    // GetShowDelay checks tab_width <= tab_min BEFORE the logarithm, so
    // a strip whose standard width collapsed to the pinned width answers
    // the floor for every tab it can hold.
    expect(hoverCardShowDelayMs(64, 64, 64)).toBe(HOVER_CARD_MIN_SHOW_DELAY_MS);
    // Only a tab WIDER than the pinned width can reach the log — where
    // it cannot interpolate (negative domain), the law rides the max.
    expect(hoverCardShowDelayMs(100, 64, 50)).toBe(
      HOVER_CARD_MAX_SHOW_DELAY_MS + HOVER_CARD_MAX_WIDTH_EXTRA_DELAY_MS,
    );
  });

  it("knows the reshow buffer is 300ms — the graze never re-arms the delay", () => {
    expect(HOVER_CARD_RESHOW_BUFFER_MS).toBe(300);
  });
});

describe("the group card's composition law", () => {
  it("headers a named group with the plural count", () => {
    // IDS_TAB_GROUPS_HOVER_CARD_HEADER, sentence case.
    expect(groupCardHeader("survival", 1)).toBe("survival (1 tab)");
    expect(groupCardHeader("survival", 3)).toBe("survival (3 tabs)");
  });

  it("headers an unnamed group with the bare count", () => {
    // IDS_TAB_GROUPS_UNNAMED_GROUP_HOVER_CARD_HEADER.
    expect(groupCardHeader(null, 1)).toBe("1 tab");
    expect(groupCardHeader("", 4)).toBe("4 tabs");
  });

  it("shows at most five members and counts the rest", () => {
    // tabs::TabGroupData::kMaxTabs = 5.
    const titles = ["a", "b", "c", "d", "e", "f", "g"];
    const { members, excess } = groupCardMembers(titles);
    expect(members).toEqual(["a", "b", "c", "d", "e"]);
    expect(excess).toBe(2);
    expect(groupCardFooterText(excess)).toBe("+ 2 More");
    expect(GROUP_CARD_MAX_TABS).toBe(5);

    // Exactly five: no footer.
    expect(groupCardMembers(titles.slice(0, 5))).toEqual({
      members: titles.slice(0, 5),
      excess: 0,
    });
  });
});

describe("the domain law", () => {
  it("is the address without its scheme", () => {
    expect(hoverCardDomain("zim://server/survival")).toBe("server/survival");
    expect(hoverCardDomain("zim://join/localhost:25565")).toBe("join/localhost:25565");
    expect(hoverCardDomain("zim://settings/")).toBe("settings/");
  });

  it("hides the line when nothing remains", () => {
    expect(hoverCardDomain("zim://")).toBeNull();
    expect(hoverCardDomain(null)).toBeNull();
    expect(hoverCardDomain(undefined)).toBeNull();
  });

  it("passes non-zim addresses through untouched", () => {
    expect(hoverCardDomain("https://example.com")).toBe("https://example.com");
  });
});

describe("the delay's consistency law", () => {
  it("reads the largest tab slot, chips excluded", () => {
    // GetShowDelay uses the largest tab so every tab of a strip answers
    // the same way; header slots are chips, not tabs.
    const slots = [
      { header: false, width: 64 },
      { header: true, width: 300 },
      { header: false, width: 200 },
      { header: false, width: 150 },
    ];
    expect(largestTabSlotWidth(slots)).toBe(200);
    expect(largestTabSlotWidth([])).toBe(0);
  });
});

describe("the slide band's law", () => {
  it("covers everything below the strip band on a horizontal strip", () => {
    // The anchor's y is shared by every tab of the band — the carrier
    // spans the width and everything under it.
    const band = hoverCardBand(false, 47, 240, { w: 1024, h: 600 });
    expect(band).toEqual({ x: 0, y: 47, w: 1024, h: 553 });
  });

  it("is the rail column plus the card's reach on the vertical rail", () => {
    const band = hoverCardBand(true, 47, 240, { w: 1024, h: 600 });
    expect(band).toEqual({
      x: 0,
      y: 0,
      w: 240 + 256 + HOVER_CARD_RAIL_REACH_MARGIN,
      h: 600,
    });
    expect(HOVER_CARD_RAIL_REACH_MARGIN).toBe(64);
  });
});
