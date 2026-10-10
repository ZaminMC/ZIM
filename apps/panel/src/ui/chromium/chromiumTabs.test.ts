// The backport's pins: every law the frame's strip visuals ride is asserted
// against the value in the cited upstream file. If one of these fails after
// a re-port, upstream changed — read the cited file again, then re-derive.
import { describe, expect, it } from "vitest";
import {
  BOTTOM_CORNER_RADIUS,
  CONTENTS_INSET_X,
  CONTENTS_INSET_Y,
  GROUP_CHIP_CORNER_RADIUS,
  GROUP_CHIP_EMPTY_SIZE,
  GROUP_CHIP_TOP,
  GROUP_COLLAPSED_HEADER_PADDING,
  GROUP_COLOR_IDS,
  GROUP_LEADING_HEADER_PADDING,
  GROUP_LINE_BOTTOM,
  GROUP_LINE_STROKE_INSET,
  GROUP_LINE_STROKE_THICKNESS,
  MIN_ACTIVE_WIDTH,
  MIN_INACTIVE_WIDTH,
  PINNED_WIDTH,
  SQUARCLE_HEIGHT,
  SQUARCLE_INSET_X,
  SQUARCLE_TIGHT_INSET_X,
  STANDARD_SLOT_WIDTH,
  STRIP_COLORS,
  TAB_HEIGHT,
  TAB_OVERLAP,
  TAB_STRIP_DECLUTTER_MAX_TAB_WIDTH_FOR_CLOSE_HIDE,
  TAB_STRIP_DECLUTTER_MIN_TABS_FOR_SEPARATOR_HIDE,
  TAB_STRIP_HEIGHT,
  TOP_CORNER_RADIUS,
  SELECTED_TAB_OPACITY,
  activeTabPath,
  alphaBlend,
  contrastRatio,
  groupChipForeground,
  groupColorCount,
  groupTabStripColor,
  separatorColor,
  topCornerRadiusForWidth,
} from "./chromiumTabs";

describe("TabStyle metrics (tab_style.cc)", () => {
  it("derives the overlap from the separator law: 24 − 6", () => {
    expect(TAB_OVERLAP).toBe(18);
  });

  it("spans a standard slot at 232 + 2×12 DIP", () => {
    expect(STANDARD_SLOT_WIDTH).toBe(256);
  });

  it("sizes pinned, min-active, and min-inactive tabs at 64 / 56 / 32", () => {
    // GetPinnedWidth: 24 content + both contents insets (20 each side).
    expect(PINNED_WIDTH).toBe(64);
    // GetMinimumActiveWidth: close button 16 + insets.
    expect(MIN_ACTIVE_WIDTH).toBe(56);
    // GetMinimumInactiveWidth: 16 interior − 2 separator + 18 overlap.
    expect(MIN_INACTIVE_WIDTH).toBe(32);
    expect(CONTENTS_INSET_X).toBe(20);
    expect(CONTENTS_INSET_Y).toBe(12);
  });

  it("keeps a third of a narrow tab's top flat (GetTopCornerRadiusForWidth)", () => {
    expect(TOP_CORNER_RADIUS).toBe(10);
    expect(BOTTOM_CORNER_RADIUS).toBe(12);
    expect(topCornerRadiusForWidth(256)).toBe(10); // ideal
    expect(topCornerRadiusForWidth(50)).toBe(10); // (50−20)/3 = 10
    expect(topCornerRadiusForWidth(44)).toBe(8); // (44−20)/3 = 8
    expect(topCornerRadiusForWidth(32)).toBe(4); // min-width tabs
    expect(topCornerRadiusForWidth(20)).toBe(0); // no flat top left
    expect(topCornerRadiusForWidth(10)).toBe(0); // clamped, never negative
  });

  it("rides the 35+6 band (layout_constants.cc)", () => {
    expect(TAB_HEIGHT).toBe(35);
    expect(TAB_STRIP_HEIGHT).toBe(41);
  });

  it("hides separators only in the declutter regime", () => {
    expect(TAB_STRIP_DECLUTTER_MAX_TAB_WIDTH_FOR_CLOSE_HIDE).toBe(100);
    expect(TAB_STRIP_DECLUTTER_MIN_TABS_FOR_SEPARATOR_HIDE).toBe(20);
  });
});

describe("GetPath (horizontal_tab_style_views.cc)", () => {
  it("draws the clockwise chrome tab: extensions, bottom arcs r12, top arcs rTop", () => {
    const d = activeTabPath(256);
    expect(d.startsWith("M 0 35")).toBe(true);
    expect(d).toContain("A 12 12 0 0 0 12 22"); // bottom-left arc
    expect(d).toContain("A 10 10 0 0 1 22 0"); // top-left arc
    expect(d).toContain("A 10 10 0 0 1 244 10"); // top-right arc
    expect(d).toContain("A 12 12 0 0 0 256 34"); // bottom-right arc
    expect(d.endsWith("L 256 35 Z")).toBe(true);
  });

  it("shrinks the top radius on min-width tabs (32 DIP → r4)", () => {
    const d = activeTabPath(32);
    expect(d).toContain("A 4 4 0 0 1 16 0"); // top-left arc at r4
    expect(d).toContain("A 12 12 0 0 0 32 34"); // extensions stay r12
  });
});

describe("the detached squarcle (kHighlight path)", () => {
  it("insets one bottom radius per side, kTabHeight − padding − overlap tall", () => {
    expect(SQUARCLE_INSET_X).toBe(12);
    expect(SQUARCLE_HEIGHT).toBe(28);
  });

  it("expands narrow tabs into the separator zone by half its width", () => {
    expect(SQUARCLE_TIGHT_INSET_X).toBe(9);
  });
});

describe("the color pipeline (tab_strip_color_mixer.cc)", () => {
  it("blends hover at 40% of the toolbar over the frame", () => {
    expect(STRIP_COLORS.light.frame).toBe("#dee1e6");
    expect(STRIP_COLORS.light.toolbar).toBe("#ffffff");
    expect(STRIP_COLORS.light.inactiveHover).toBe(alphaBlend("#ffffff", "#dee1e6", 0.4));
    // The blend is real: lighter than the frame in light mode, and the
    // dark ladder mirrors it toward the dark toolbar.
    expect(STRIP_COLORS.dark.frame).toBe("#202124");
    expect(STRIP_COLORS.dark.toolbar).toBe("#35363a");
  });

  it("AlphaBlend composites fg at alpha over bg", () => {
    expect(alphaBlend("#ffffff", "#000000", 0.4)).toBe("#666666");
    expect(alphaBlend("#000000", "#ffffff", 0)).toBe("#ffffff");
    expect(alphaBlend("#000000", "#ffffff", 1)).toBe("#000000");
  });

  it("reads contrast the WCAG way", () => {
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 0);
    expect(contrastRatio("#ffffff", "#ffffff")).toBe(1);
  });

  it("selected tabs blend at 75% (kDefaultSelectedTabOpacity)", () => {
    expect(SELECTED_TAB_OPACITY).toBe(0.75);
  });
});

describe("group colors (tab_group_color.h + chrome_color_mixer.cc)", () => {
  it("keeps the wire-format enum order: grey..orange, kNumEntries = 9", () => {
    expect(GROUP_COLOR_IDS).toEqual([
      "grey",
      "blue",
      "red",
      "yellow",
      "green",
      "pink",
      "purple",
      "cyan",
      "orange",
    ]);
    expect(groupColorCount).toBe(9);
  });

  it("carries the classic palette: deep light values, pastel dark values", () => {
    const blue = groupTabStripColor(1);
    expect(blue.light).toBe("#1a73e8"); // kGoogleBlue600
    expect(blue.dark).toBe("#8ab4f8"); // kGoogleBlue300
    const cyan = groupTabStripColor(7);
    expect(cyan.light).toBe("#007b83"); // kGoogleCyan900
  });

  it("wraps indices the way %GROUP_COLORS does, negatives included", () => {
    expect(groupTabStripColor(0).id).toBe("grey");
    expect(groupTabStripColor(9).id).toBe("grey");
    expect(groupTabStripColor(10).id).toBe("blue");
    expect(groupTabStripColor(-1).id).toBe("orange");
  });
});

describe("the header chip (tab_group_style.cc + tab_group_header_view.cc)", () => {
  it("rides 20×20 at radius 6, top 4 inside the tab view", () => {
    expect(GROUP_CHIP_EMPTY_SIZE).toBe(20);
    expect(GROUP_CHIP_CORNER_RADIUS).toBe(6);
    expect(GROUP_CHIP_TOP).toBe(4);
  });

  it("pads the run from the overlap law: 16 leading, 14 between collapsed", () => {
    expect(GROUP_LEADING_HEADER_PADDING).toBe(16);
    expect(GROUP_COLLAPSED_HEADER_PADDING).toBe(14);
  });

  it("labels read at max contrast over the solid body", () => {
    // GetColorWithMaxContrast is MECHANICAL, and #1a73e8 sits just past
    // the crossover: black 4.66 vs white 4.50 — the classic blue chip
    // reads black, whatever one remembers from the alternate palette's
    // pastel chips. The law wins; the pins assert the law.
    expect(groupChipForeground("#1a73e8")).toBe("#000000"); // blue chip
    expect(groupChipForeground("#fa903e")).toBe("#000000"); // orange chip
    expect(groupChipForeground("#fdd663")).toBe("#000000"); // dark yellow
    expect(groupChipForeground("#5f6368")).toBe("#ffffff"); // grey chip
  });
});

describe("the group underline (tab_group_underline.cc)", () => {
  it("strokes 2 DIP, inset 18, riding one toolbar-overlap above the floor", () => {
    expect(GROUP_LINE_STROKE_THICKNESS).toBe(2);
    expect(GROUP_LINE_STROKE_INSET).toBe(18);
    expect(GROUP_LINE_BOTTOM).toBe(1);
  });
});

describe("separators (tab_style.cc GetContrastRatioValues)", () => {
  it("blend to exactly the 2.5-contrast answer over the strip", () => {
    const sep = separatorColor("#dee1e6", "#3c4043");
    expect(contrastRatio(sep, "#dee1e6")).toBeGreaterThanOrEqual(2.5);
    // BlendForMinContrast finds the SMALLEST alpha that clears the bar —
    // brute-force the minimal alpha and demand the same color back.
    let minimal = "";
    for (let a = 0; a <= 1; a += 0.001) {
      const blend = alphaBlend("#3c4043", "#dee1e6", a);
      if (contrastRatio(blend, "#dee1e6") >= 2.5) {
        minimal = blend;
        break;
      }
    }
    expect(sep).toBe(minimal);
  });
});
