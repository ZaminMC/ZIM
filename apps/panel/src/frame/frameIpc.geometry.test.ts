// The view-side geometry mirrors hold the same laws the model's own
// tests hold: the drop index follows slot centers over REAL tabs only,
// the scroll never outruns the layout's overflow, the reveal shifts just
// enough. The host's drop verdicts (layout::drop_index) stay
// authoritative — these keep the frame's live preview honest between
// round-trips.

import { describe, expect, it } from "vitest";
import {
  demoLayoutStripVertical,
  dropIndexFromSlots,
  dropIndexFromSlotsVertical,
  insertAtDropIndex,
  revealSlot,
  stripScroll,
  stripScrollVertical,
  type Slot,
} from "./frameIpc";

const slot = (id: number, x: number, width: number, header = false): Slot => ({
  id,
  x,
  width,
  y: 0,
  height: 35,
  pinned: false,
  closing: false,
  header,
});

const TABS = [slot(1, 6, 256), slot(2, 244, 256), slot(3, 482, 256)];

describe("dropIndexFromSlots", () => {
  it("follows the slot centers", () => {
    // Center of tab 1 is 6+128=134; a pointer left of it opens index 0.
    expect(dropIndexFromSlots(TABS, 100)).toBe(0);
    expect(dropIndexFromSlots(TABS, 200)).toBe(1);
    expect(dropIndexFromSlots(TABS, 5000)).toBe(3);
  });

  it("never lands on a group chip", () => {
    const withChip = [slot(7, 6, 70, true), ...TABS];
    expect(dropIndexFromSlots(withChip, 30)).toBe(0);
  });
});

describe("insertAtDropIndex — the block landing (move_block's mirror)", () => {
  const row = (id: number, pinned: boolean) => ({ id, pinned });

  it("lands a multi-tab block in one piece", () => {
    // rest = the strip minus the lifted block [4, 5]; the block lands
    // between 2 and 3 as a contiguous run.
    const rest = [row(1, false), row(2, false), row(3, false)];
    const out = insertAtDropIndex(rest, [row(4, false), row(5, false)], 2);
    expect(out.map((t) => t.id)).toEqual([1, 2, 4, 5, 3]);
  });

  it("an unpinned block never enters the pinned prefix", () => {
    const rest = [row(1, true), row(2, true), row(3, false)];
    const out = insertAtDropIndex(rest, [row(4, false), row(5, false)], 0);
    expect(out.map((t) => t.id)).toEqual([1, 2, 4, 5, 3]);
  });

  it("a pinned block clamps inside the pinned region", () => {
    const rest = [row(1, true), row(2, false), row(3, false)];
    const out = insertAtDropIndex(rest, [row(4, true), row(5, true)], 5);
    expect(out.map((t) => t.id)).toEqual([1, 4, 5, 2, 3]);
  });

  it("a single tab rides the same law (the old contract)", () => {
    const rest = [row(1, false), row(2, false)];
    const out = insertAtDropIndex(rest, row(3, false), 1);
    expect(out.map((t) => t.id)).toEqual([1, 3, 2]);
  });
});

describe("stripScroll", () => {
  it("does not scroll when the run fits the reserve", () => {
    const { max } = stripScroll(TABS, 1600, 0);
    expect(max).toBe(0);
  });

  it("scrolls exactly to fit the overflow", () => {
    // 20 minimum tabs (32 each, overlap 18) in a 320px strip: end =
    // 6 + 19×14 + 32 = 304 — far past the reserve.
    const many: Slot[] = [];
    for (let i = 0; i < 20; i++) many.push(slot(i + 1, 6 + i * 14, 32));
    const { max, value } = stripScroll(many, 320, 10_000);
    expect(max).toBeGreaterThan(0);
    expect(value).toBe(max);
    // Scrolled to the end, the last tab's chain end must clear the
    // caption area (width - 138) — the + may overlap the tail, the
    // caption buttons may not.
    const end = many[many.length - 1]!.x + many[many.length - 1]!.width - 18;
    expect(end - max).toBeLessThanOrEqual(320 - 138 + 0.01);
  });
});

describe("revealSlot", () => {
  it("stays put while the active tab is visible", () => {
    expect(revealSlot({ x: 300, width: 256 }, 1600, 20, 500)).toBe(20);
  });

  it("shifts just enough when the active tab is past the limit", () => {
    // limit = 1600-174 = 1426; the tab ends at 1500 → shift 74.
    expect(revealSlot({ x: 1244, width: 256 }, 1600, 0, 500)).toBe(74);
  });

  it("shifts to the slot's left edge when it is scrolled past", () => {
    expect(revealSlot({ x: 400, width: 256 }, 1600, 500, 800)).toBe(400);
  });

  it("never outruns the strip's own maximum", () => {
    expect(revealSlot({ x: 5000, width: 256 }, 1600, 0, 100)).toBe(100);
  });
});

// -- §54: the rail's own laws (ADR-0026) --------------------------------------

const vTab = (id: number, y: number): Slot => ({
  id,
  x: 6,
  width: 228,
  y,
  height: 35,
  pinned: false,
  closing: false,
  header: false,
});

describe("demoLayoutStripVertical", () => {
  const stripArg = {
    strip_width: 1280,
    tabs: [
      { id: 1, pinned: true, group: null },
      { id: 2, pinned: false, group: null },
      { id: 3, pinned: false, group: 7 },
      { id: 4, pinned: false, group: 7 },
    ],
    groups: [{ id: 7, label: "survival", collapsed: false }],
    active: 2,
  };

  it("stacks full-width rows from the padding with the row gap", () => {
    const slots = demoLayoutStripVertical(stripArg, 240);
    // Chip row (group 7 leads at tab 3), then four tab rows.
    expect(slots).toHaveLength(5);
    const stride = 35 + 4;
    slots.forEach((s, i) => {
      expect(s.x).toBe(6);
      expect(s.width).toBe(228);
      expect(s.y).toBe(6 + i * stride);
      expect(s.height).toBe(35);
    });
  });

  it("leads a group with its chip row and collapses to it", () => {
    const collapsed = demoLayoutStripVertical(
      { ...stripArg, groups: [{ id: 7, label: "survival", collapsed: true }] },
      240,
    );
    const chip = collapsed.find((s) => s.header);
    expect(chip).toBeDefined();
    expect(collapsed.filter((s) => !s.header).map((s) => s.id)).toEqual([1, 2]);
  });

  it("keeps model order — the rail does not reorder pinned tabs", () => {
    const slots = demoLayoutStripVertical(stripArg, 240);
    expect(slots.filter((s) => !s.header).map((s) => s.id)).toEqual([1, 2, 3, 4]);
  });
});

describe("dropIndexFromSlotsVertical", () => {
  const rows = [vTab(1, 6), vTab(2, 45), vTab(3, 84)];

  it("follows the row centers", () => {
    expect(dropIndexFromSlotsVertical(rows, 10)).toBe(0);
    expect(dropIndexFromSlotsVertical(rows, 70)).toBe(2);
    expect(dropIndexFromSlotsVertical(rows, 5000)).toBe(3);
  });

  it("never lands on a chip row", () => {
    const withChip = [vTab(7, 6), ...rows];
    expect(dropIndexFromSlotsVertical(withChip, 20)).toBe(0);
  });
});

describe("stripScrollVertical", () => {
  it("does not scroll while the stack fits the lane", () => {
    const { max } = stripScrollVertical([vTab(1, 6), vTab(2, 45)], 600, 0);
    expect(max).toBe(0);
  });

  it("scrolls to fit the overflow plus the + row's reserve", () => {
    const many: Slot[] = [];
    for (let i = 0; i < 20; i++) many.push(vTab(i + 1, 6 + i * 39));
    const { max, value } = stripScrollVertical(many, 300, 10_000);
    expect(max).toBeGreaterThan(0);
    expect(value).toBe(max);
  });
});
