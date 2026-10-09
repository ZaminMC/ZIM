// The view-side geometry mirrors hold the same laws the model's own
// tests hold: the drop index follows slot centers over REAL tabs only,
// the scroll never outruns the layout's overflow, the reveal shifts just
// enough. The host's drop verdicts (layout::drop_index) stay
// authoritative — these keep the frame's live preview honest between
// round-trips.

import { describe, expect, it } from "vitest";
import { dropIndexFromSlots, revealSlot, stripScroll, type Slot } from "./frameIpc";

const slot = (id: number, x: number, width: number, header = false): Slot => ({
  id,
  x,
  width,
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
