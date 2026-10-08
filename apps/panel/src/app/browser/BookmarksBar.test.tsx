// The bookmark bar (§55): chips navigate the active tab (through the
// tabs store, so §61 singleton focus is inherited), Ctrl/Cmd+click and
// middle-click open a NEW tab, and the context menu carries rename and
// remove. An empty bar renders nothing — Chrome's own honesty.

import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { vi } from "vitest";
import { afterEach, describe, expect, it } from "vitest";
import { BookmarksBar } from "./BookmarksBar";
import { useBookmarks } from "../../state/bookmarks";
import { useTabs } from "../../state/tabs";

const survival = { kind: "server" as const, serverId: "survival" };
const settings = { kind: "settings" as const };

function seed() {
  useBookmarks.setState({ items: [], barVisible: true });
  useBookmarks.getState().add(survival, "Survival");
  useBookmarks.getState().add(settings, "Panel settings");
}

function freshTabs() {
  useTabs.setState({ tabs: [], activeId: null });
  useTabs.getState().newTab();
}

describe("BookmarksBar", () => {
  afterEach(() => {
    cleanup();
    useBookmarks.setState({ items: [], barVisible: true });
    useTabs.setState({ tabs: [], activeId: null });
  });

  it("renders nothing while the bar is hidden", () => {
    seed();
    useBookmarks.setState({ barVisible: false });
    render(<BookmarksBar />);
    expect(screen.queryByRole("toolbar", { name: "Bookmarks" })).toBeNull();
  });

  it("renders nothing while there is nothing to show", () => {
    useBookmarks.setState({ items: [], barVisible: true });
    render(<BookmarksBar />);
    expect(screen.queryByRole("toolbar", { name: "Bookmarks" })).toBeNull();
  });

  it("a click navigates the active tab to the bookmark's destination", () => {
    seed();
    freshTabs();
    render(<BookmarksBar />);
    fireEvent.click(screen.getByRole("button", { name: "Survival" }));
    const state = useTabs.getState();
    const active = state.tabs.find((t) => t.id === state.activeId);
    expect(active?.history[active.historyIndex]).toEqual(survival);
  });

  it("Ctrl+click opens a NEW tab instead of navigating", () => {
    seed();
    freshTabs();
    render(<BookmarksBar />);
    const before = useTabs.getState().tabs.length;
    fireEvent.click(screen.getByRole("button", { name: "Panel settings" }), {
      ctrlKey: true,
    });
    const state = useTabs.getState();
    expect(state.tabs.length).toBe(before + 1);
    const active = state.tabs.find((t) => t.id === state.activeId);
    expect(active?.history[0]).toEqual(settings);
  });

  it("middle-click opens a new tab too", () => {
    seed();
    freshTabs();
    render(<BookmarksBar />);
    const before = useTabs.getState().tabs.length;
    fireEvent(
      screen.getByRole("button", { name: "Survival" }),
      new MouseEvent("auxclick", { button: 1, bubbles: true }),
    );
    expect(useTabs.getState().tabs.length).toBe(before + 1);
  });

  it("the context menu renames and removes", () => {
    seed();
    render(<BookmarksBar />);
    fireEvent.contextMenu(screen.getByRole("button", { name: "Survival" }));
    const promptSpy = vi
      .spyOn(window, "prompt")
      .mockReturnValue("SMP");
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename…" }));
    expect(useBookmarks.getState().items[0]?.label).toBe("SMP");
    promptSpy.mockRestore();

    fireEvent.contextMenu(screen.getByRole("button", { name: "SMP" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Remove" }));
    expect(useBookmarks.getState().items.map((i) => i.label)).toEqual(["Panel settings"]);
  });

  it("the context menu opens a new tab", () => {
    seed();
    freshTabs();
    render(<BookmarksBar />);
    const before = useTabs.getState().tabs.length;
    fireEvent.contextMenu(screen.getByRole("button", { name: "Survival" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Open in new tab/ }));
    expect(useTabs.getState().tabs.length).toBe(before + 1);
  });
});
