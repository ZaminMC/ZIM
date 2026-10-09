// The demo fixture's layout mirror, held to the model's law. The mirror
// exists so the browser can wear the real strip (UI iteration without
// the desktop host) — the moment it drifts from shell/layout.rs, the
// fixture previews a UI the host will never render. The case below is
// the regression that already bit once: a group chip's width dropped
// out of the overflow budget, and the fixture slid tabs under the
// new-tab button and the caption area while the host stayed correct.

import { describe, expect, it } from "vitest";
import { demoLayoutStrip } from "./frameIpc";

const NEW_TAB_W = 36;
const CONTROLS_W = 138;
const LEADING = 6;

function stripOf(count: number, stripWidth: number, group?: { id: number; members: number[] }) {
  return {
    strip_width: stripWidth,
    tabs: Array.from({ length: count }, (_, i) => ({
      id: i + 1,
      pinned: false,
      group: group?.members.includes(i + 1) ? group.id : null,
    })),
    groups: group ? [{ id: group.id, label: "survival", collapsed: false }] : [],
    active: 1,
  };
}

describe("demoLayoutStrip (the fixture's mirror of layout.rs)", () => {
  it("never slides a slot under the new-tab button or the caption area", () => {
    // layout.rs's own caption law, restated for the mirror: 30 tabs on a
    // narrow strip overflow hard, and every slot must still end before
    // the window controls' left edge.
    const strip = stripOf(30, 900);
    const reserve = 900 - CONTROLS_W - NEW_TAB_W;
    for (const slot of demoLayoutStrip(strip)) {
      expect(slot.x + slot.width).toBeLessThanOrEqual(reserve + 0.01);
    }
  });

  it("counts the group chip toward the strip's span", () => {
    // The chip is a slot like any other: with it, the leftover budget
    // shrinks and the last tab's width follows; without it, the same
    // strip has more room. The mirror once ignored the chip's width and
    // overfilled the strip by exactly the chip's span.
    const withGroup = demoLayoutStrip(stripOf(30, 900, { id: 7, members: [2, 3] }));
    const withoutGroup = demoLayoutStrip(stripOf(30, 900));
    const chip = withGroup.find((s) => s.header);
    expect(chip).toBeTruthy();
    const lastWith = Math.max(...withGroup.map((s) => s.x + s.width));
    const lastWithout = Math.max(...withoutGroup.map((s) => s.x + s.width));
    expect(lastWith).toBeLessThanOrEqual(900 - CONTROLS_W - NEW_TAB_W + 0.01);
    expect(lastWith).toBeLessThan(lastWithout + 0.01);
  });

  it("keeps the leading inset and the pinned lane's fixed width", () => {
    const strip = {
      strip_width: 1400,
      tabs: [
        { id: 1, pinned: true, group: null },
        { id: 2, pinned: false, group: null },
      ],
      groups: [],
      active: 2,
    };
    const slots = demoLayoutStrip(strip);
    expect(slots[0]?.x).toBe(LEADING);
    expect(slots[0]?.width).toBe(40); // pinned_width()
    expect(slots[1]?.width).toBe(256); // standard_width()
  });
});
