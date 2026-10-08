// Bookmarks store (§55): identity discipline — one bookmark per
// destination, the typed destination (not a string) is what's kept, and
// torn storage drops what the shell could not open.

import { describe, expect, it, beforeEach } from "vitest";
import { bookmarkKeyOf, isBookmarked, useBookmarks } from "./bookmarks";
import type { Destination } from "./destinations";

const survival: Destination = { kind: "server", serverId: "survival" };
const settings: Destination = { kind: "settings" };
const fleet: Destination = { kind: "servers" };

function reset() {
  useBookmarks.setState({ items: [], barVisible: true });
  localStorage.removeItem("zamin-panel.bookmarks");
}

describe("bookmarks store", () => {
  beforeEach(reset);

  it("adds a bookmark and keeps the typed destination", () => {
    useBookmarks.getState().add(survival, "Survival");
    const item = useBookmarks.getState().items[0];
    expect(item?.destination).toEqual(survival);
    expect(item?.label).toBe("Survival");
  });

  it("one destination holds one bookmark — a second add is a no-op", () => {
    useBookmarks.getState().add(survival, "Survival");
    useBookmarks.getState().add({ kind: "server", serverId: "survival" }, "Renamed");
    expect(useBookmarks.getState().items).toHaveLength(1);
    expect(useBookmarks.getState().items[0]?.label).toBe("Survival");
  });

  it("remove and removeDestination both drop the row", () => {
    useBookmarks.getState().add(survival, "Survival");
    useBookmarks.getState().add(settings, "Settings");
    const first = useBookmarks.getState().items[0];
    if (!first) throw new Error("seed failed");
    useBookmarks.getState().removeDestination(settings);
    expect(useBookmarks.getState().items.map((i) => i.id)).toEqual([first.id]);
    useBookmarks.getState().remove(first.id);
    expect(useBookmarks.getState().items).toHaveLength(0);
  });

  it("rename keeps the destination and trims the label", () => {
    useBookmarks.getState().add(survival, "Survival");
    const item = useBookmarks.getState().items[0];
    if (!item) throw new Error("seed failed");
    useBookmarks.getState().rename(item.id, "  SMP  ");
    expect(useBookmarks.getState().items[0]?.label).toBe("SMP");
    expect(useBookmarks.getState().items[0]?.destination).toEqual(survival);
  });

  it("move reorders without dropping anyone", () => {
    useBookmarks.getState().add(survival, "Survival");
    useBookmarks.getState().add(settings, "Settings");
    useBookmarks.getState().add(fleet, "Servers");
    const [a, , c] = useBookmarks.getState().items;
    if (!a || !c) throw new Error("seed failed");
    useBookmarks.getState().move(a.id, 2);
    expect(useBookmarks.getState().items.map((i) => i.label)).toEqual(["Settings", "Servers", "Survival"]);
    useBookmarks.getState().move(c.id, 99); // clamped
    expect(useBookmarks.getState().items.map((i) => i.label)).toEqual(["Settings", "Survival", "Servers"]);
  });

  it("the bar toggles", () => {
    expect(useBookmarks.getState().barVisible).toBe(true);
    useBookmarks.getState().toggleBar();
    expect(useBookmarks.getState().barVisible).toBe(false);
  });

  it("isBookmarked answers by destination identity", () => {
    useBookmarks.getState().add(survival, "Survival");
    expect(isBookmarked(useBookmarks.getState().items, survival)).toBe(true);
    expect(isBookmarked(useBookmarks.getState().items, settings)).toBe(false);
  });

  it("bookmarkKeyOf preserves destination identity across two rows", () => {
    useBookmarks.getState().add(survival, "Survival");
    const item = useBookmarks.getState().items[0];
    if (!item) throw new Error("seed failed");
    expect(bookmarkKeyOf(item)).toBe("server:survival");
  });

  it("persisted torn rows are dropped, not rendered", async () => {
    localStorage.setItem(
      "zamin-panel.bookmarks",
      JSON.stringify({
        state: {
          items: [
            { id: "bm-1", label: "Ok", destination: survival, addedAt: 1 },
            { id: "bm-2", label: "No destination", addedAt: 2 },
            { id: "bm-3", label: "Garbage" },
            "not an object",
          ],
          barVisible: false,
        },
        version: 1,
      }),
    );
    await useBookmarks.persist.rehydrate();
    const state = useBookmarks.getState();
    // The well-formed row survives; the torn shapes never render.
    expect(state.items.map((i) => i.label)).toEqual(["Ok"]);
    expect(state.barVisible).toBe(false);
  });
});
