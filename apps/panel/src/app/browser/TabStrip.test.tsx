// The tab strip's chrome (ADR-0016): pinned tabs render compact and
// refuse the accidental close, the context menu carries the §48 verbs
// with real machinery (and honest reserved rooms for the rest), and
// groups behave like browser groups — a collapsible chip, never a folder.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { TabStrip } from "./TabStrip";
import { useServers } from "../../state/servers";
import { activeKeyOf, tabKeyOf, useTabs, type Tab } from "../../state/tabs";

const server = (id: string) => ({ kind: "server", serverId: id }) as const;

let seq = 0;
const tab = (
  history: Tab["history"],
  historyIndex = history.length - 1,
  extra: Partial<Tab> = {},
): Tab => ({
  id: `seed${++seq}`,
  history,
  historyIndex,
  reloadToken: 0,
  ...extra,
});

const seed = (tabs: Tab[], activeIndex = 0) => {
  useTabs.setState({
    tabs,
    activeId: tabs[activeIndex]!.id,
    discoveryQuery: null,
    recentlyClosed: [],
    groups: {},
  });
};

const alphaEntry = {
  serverId: "alpha",
  displayName: "Alpha",
  state: "running",
  port: 25565,
} as const;

beforeEach(() => {
  seq = 0;
  useServers.setState({ servers: { alpha: alphaEntry }, crashes: {} });
  seed([tab([{ kind: "new" }])]);
});

afterEach(cleanup);

const tabByTitle = (title: string) =>
  screen.getAllByRole("tab").find((el) => el.getAttribute("title") === title)!;
const openMenuOn = (el: HTMLElement) =>
  fireEvent.contextMenu(el, { clientX: 10, clientY: 10 });
const menuItem = (name: string | RegExp) => screen.getByRole("menuitem", { name });
/** React's onAuxClick needs a real auxclick event — fireEvent has no helper. */
const auxClick = (el: HTMLElement, button: number) =>
  fireEvent(el, new MouseEvent("auxclick", { button, bubbles: true }));

describe("pinning (§52)", () => {
  it("renders the pinned tab compact — icon only, no accidental close", () => {
    seed([tab([server("alpha")], 0, { pinned: true }), tab([{ kind: "servers" }])]);
    render(<TabStrip />);
    // The compact tab shows no title text and no close button.
    expect(screen.queryByText("Alpha")).toBeNull();
    expect(screen.queryByRole("button", { name: "Close Alpha" })).toBeNull();
    // The tooltip still tells the operator what it is.
    expect(tabByTitle("Alpha")).toBeTruthy();
    expect(tabByTitle("Servers")).toBeTruthy();
    expect(tabByTitle("Close Alpha")).toBeUndefined(); // sanity on the finder
  });

  it("ignores the middle click (the accidental close), keeps the explicit one", () => {
    seed([tab([server("alpha")], 0, { pinned: true }), tab([{ kind: "servers" }])]);
    render(<TabStrip />);
    auxClick(tabByTitle("Alpha"), 1);
    expect(useTabs.getState().tabs).toHaveLength(2);

    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("Close"));
    expect(useTabs.getState().tabs).toHaveLength(1);
  });

  it("pins and unpins through the menu", () => {
    seed([tab([server("alpha")]), tab([{ kind: "servers" }])]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("Pin"));
    expect(useTabs.getState().tabs[0]!.pinned).toBe(true);
    expect(tabKeyOf(useTabs.getState().tabs[0]!)).toBe("server:alpha");

    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("Unpin"));
    expect(useTabs.getState().tabs.some((t) => t.pinned)).toBe(false);
  });
});

describe("the §48 menu", () => {
  it("duplicates the view next to the original", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")])]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("Duplicate"));
    const s = useTabs.getState();
    expect(s.tabs.map(tabKeyOf)).toEqual(["servers", "server:alpha", "server:alpha"]);
    expect(activeKeyOf(s)).toBe("server:alpha");
  });

  it("opens a new tab to the right of the menu's tab", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")])]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("New tab to the right"));
    expect(useTabs.getState().tabs.map(tabKeyOf)).toEqual(["servers", "server:alpha", "new"]);
    expect(activeKeyOf(useTabs.getState())).toBe("new");
  });

  it("reopens a closed tab, and admits when the memory is empty", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")])]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    expect((menuItem("Reopen closed tab") as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(menuItem("Close"));
    expect(useTabs.getState().tabs).toHaveLength(1);
    openMenuOn(tabByTitle("Servers"));
    expect((menuItem("Reopen closed tab") as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(menuItem("Reopen closed tab"));
    expect(useTabs.getState().tabs.map(tabKeyOf)).toEqual(["servers", "server:alpha"]);
  });

  it("keeps the reserved rooms honest — named, not faked", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")])]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    for (const reserved of [
      "Mute",
      "Move tab to new window",
      "Share tab with Dutchmen",
      "Show tabs vertically",
    ]) {
      expect((menuItem(reserved) as HTMLButtonElement).disabled).toBe(true);
    }
  });
});

describe("groups (§49)", () => {
  it("groups a tab; the chip arrives with the palette's first color", () => {
    seed([tab([server("alpha")]), tab([{ kind: "servers" }])]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("Add tab to new group…"));
    const s = useTabs.getState();
    const gid = s.tabs[0]!.groupId!;
    expect(s.groups[gid]!.label).toBe("New group");
    expect(screen.getByRole("button", { name: /New group/ })).toBeTruthy();
  });

  it("collapses on a chip click — members hide, the count shows", () => {
    seed([tab([server("alpha")]), tab([{ kind: "servers" }])]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("Add tab to new group…"));
    openMenuOn(tabByTitle("Servers"));
    fireEvent.click(menuItem(/^Move to group/));
    fireEvent.click(menuItem("New group"));

    const chip = screen.getByRole("button", { name: /New group/ });
    fireEvent.click(chip);
    const s = useTabs.getState();
    const [gid] = Object.keys(s.groups);
    expect(s.groups[gid!]!.collapsed).toBe(true);
    // Both members hide; the chip alone remains, wearing the count.
    expect(screen.queryByRole("tab")).toBeNull();
    expect(screen.queryByText("Alpha")).toBeNull();
    expect(screen.queryByText("Servers")).toBeNull();
    expect(screen.getByText("2")).toBeTruthy();
  });

  it("renames through the chip's double click", () => {
    seed([tab([server("alpha")])]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("Add tab to new group…"));
    const chip = screen.getByRole("button", { name: /New group/ });
    fireEvent.doubleClick(chip);
    const input = screen.getByLabelText<HTMLInputElement>("Group name");
    fireEvent.change(input, { target: { value: "Survival" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByRole("button", { name: /Survival/ })).toBeTruthy();
    const s = useTabs.getState();
    const [gid] = Object.keys(s.groups);
    expect(s.groups[gid!]!.label).toBe("Survival");
  });

  it("expands back and removes members; the emptied group dissolves", () => {
    seed([tab([server("alpha")]), tab([{ kind: "servers" }])]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("Add tab to new group…"));
    openMenuOn(tabByTitle("Servers"));
    fireEvent.click(menuItem(/^Move to group/));
    fireEvent.click(menuItem("New group"));

    // Collapse, then expand again.
    fireEvent.click(screen.getByRole("button", { name: /New group/ }));
    fireEvent.click(screen.getByRole("button", { name: /New group/ }));
    expect(tabByTitle("Alpha")).toBeTruthy();

    // Removing one member leaves the group with Alpha.
    openMenuOn(tabByTitle("Servers"));
    fireEvent.click(menuItem("Remove from group"));
    expect(screen.getByRole("button", { name: /New group/ })).toBeTruthy();

    // Removing the last member dissolves the group; the chip goes with it.
    openMenuOn(tabByTitle("Alpha"));
    fireEvent.click(menuItem("Remove from group"));
    expect(useTabs.getState().groups).toEqual({});
    expect(screen.queryByRole("button", { name: /New group/ })).toBeNull();
  });

  it("keeps a pinned tab out of groups", () => {
    seed([tab([server("alpha")], 0, { pinned: true })]);
    render(<TabStrip />);
    openMenuOn(tabByTitle("Alpha"));
    expect((menuItem("Add tab to new group…") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(menuItem("Add tab to new group…"));
    expect(useTabs.getState().groups).toEqual({});
  });
});
