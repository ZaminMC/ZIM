// The tab store's contract (ADR-0015): singleton identity, per-tab
// destination history, honest reload tokens, neighbor-aware close, and
// storage that cannot rehydrate into a broken shell.

import { beforeEach, describe, expect, it } from "vitest";
import {
  canForward,
  sanitizeTabs,
  tabDestination,
  tabKeyOf,
  useTabs,
  type Tab,
} from "./tabs";

const server = (id: string) => ({ kind: "server", serverId: id }) as const;
const tab = (history: Tab["history"], historyIndex = history.length - 1, reloadToken = 0): Tab => ({
  history,
  historyIndex,
  reloadToken,
});

const reset = () =>
  useTabs.setState({ tabs: [tab([{ kind: "new" }])], activeKey: "new", discoveryQuery: null });

beforeEach(reset);

describe("navigation and identity", () => {
  it("opens with one new tab", () => {
    reset();
    expect(useTabs.getState().tabs).toHaveLength(1);
    expect(tabDestination(useTabs.getState().tabs[0]!)).toEqual({ kind: "new" });
    expect(useTabs.getState().activeKey).toBe("new");
  });

  it("navigates the active tab and keeps history honest", () => {
    const { navigate } = useTabs.getState();
    navigate({ kind: "servers" });
    let s = useTabs.getState();
    expect(s.activeKey).toBe("servers");
    expect(tabDestination(s.tabs[0]!)).toEqual({ kind: "servers" });

    navigate(server("alpha"));
    s = useTabs.getState();
    expect(s.activeKey).toBe("server:alpha");
    expect(s.tabs[0]!.history).toEqual([{ kind: "new" }, { kind: "servers" }, server("alpha")]);

    // Back returns to the fleet; the tab's key follows its destination.
    useTabs.getState().back();
    s = useTabs.getState();
    expect(s.activeKey).toBe("servers");
    expect(canForward(s.tabs[0]!)).toBe(true);

    // Forward returns to the server again.
    useTabs.getState().forward();
    expect(useTabs.getState().activeKey).toBe("server:alpha");
  });

  it("focuses a destination that already rests in another tab, never duplicates", () => {
    useTabs.setState({
      tabs: [tab([{ kind: "new" }, server("alpha")], 1), tab([{ kind: "servers" }])],
      activeKey: "servers",
    });
    useTabs.getState().navigate(server("alpha"));
    const s = useTabs.getState();
    expect(s.activeKey).toBe("server:alpha");
    expect(s.tabs, "no third tab was created").toHaveLength(2);
    // The fleet tab's history was not polluted by the focus.
    expect(s.tabs[1]!.history).toEqual([{ kind: "servers" }]);
  });

  it("truncates the forward tail when navigating from the past", () => {
    useTabs.setState({ tabs: [tab([{ kind: "new" }, { kind: "servers" }, server("alpha")], 0)], activeKey: "new" });
    useTabs.getState().navigate(server("beta"));
    const s = useTabs.getState();
    expect(s.tabs[0]!.history).toEqual([{ kind: "new" }, server("beta")]);
    expect(canForward(s.tabs[0]!)).toBe(false);
  });

  it("keeps the new tab a singleton", () => {
    useTabs.getState().navigate({ kind: "servers" });
    useTabs.getState().newTab();
    let s = useTabs.getState();
    expect(s.tabs).toHaveLength(2);
    expect(s.activeKey).toBe("new");

    useTabs.getState().newTab();
    s = useTabs.getState();
    expect(s.tabs, "the resting new tab was focused, not duplicated").toHaveLength(2);
  });

  it("carries a discovery query into the new tab and clears it on plain navigation", () => {
    useTabs.getState().navigate({ kind: "servers" });
    useTabs.getState().newTab("survival");
    expect(useTabs.getState().discoveryQuery).toBe("survival");
    useTabs.getState().navigate(server("alpha"));
    expect(useTabs.getState().discoveryQuery).toBeNull();
  });
});

describe("reload", () => {
  it("bumps only the active tab's token", () => {
    useTabs.setState({
      tabs: [tab([{ kind: "servers" }], 0, 3), tab([server("alpha")])],
      activeKey: "servers",
    });
    useTabs.getState().reload();
    const s = useTabs.getState();
    expect(s.tabs[0]!.reloadToken).toBe(4);
    expect(s.tabs[1]!.reloadToken).toBe(0);
  });
});

describe("closing", () => {
  it("activates the right neighbor, then the left when none", () => {
    useTabs.setState({
      tabs: [tab([{ kind: "servers" }]), tab([server("alpha")]), tab([server("beta")])],
      activeKey: "server:alpha",
    });
    useTabs.getState().close("server:alpha");
    expect(useTabs.getState().activeKey).toBe("server:beta");

    useTabs.getState().close("server:beta");
    expect(useTabs.getState().activeKey).toBe("servers");
  });

  it("closing the last tab leaves a fresh new tab", () => {
    useTabs.setState({ tabs: [tab([server("alpha")])], activeKey: "server:alpha" });
    useTabs.getState().close("server:alpha");
    const s = useTabs.getState();
    expect(s.tabs).toHaveLength(1);
    expect(s.activeKey).toBe("new");
  });

  it("closeToTheRight and closeOthers keep their promise", () => {
    useTabs.setState({
      tabs: [tab([{ kind: "servers" }]), tab([server("alpha")]), tab([server("beta")])],
      activeKey: "server:beta",
    });
    useTabs.getState().closeToTheRight("server:alpha");
    let s = useTabs.getState();
    expect(s.tabs.map(tabKeyOf)).toEqual(["servers", "server:alpha"]);
    expect(s.activeKey, "the closed active tab hands over to its left neighbor").toBe(
      "server:alpha",
    );

    useTabs.getState().closeOthers("servers");
    s = useTabs.getState();
    expect(s.tabs.map(tabKeyOf)).toEqual(["servers"]);
    expect(s.activeKey).toBe("servers");
  });
});

describe("storage hygiene", () => {
  it("rehydrates a real session", () => {
    // sanitizeTabs is the merge guard; the happy path keeps everything.
    const raw = [
      { history: [{ kind: "servers" }, server("alpha")], historyIndex: 1, reloadToken: 2 },
    ];
    const tabs = sanitizeTabs(raw);
    expect(tabs).toHaveLength(1);
    expect(tabs![0]!.reloadToken).toBe(2);
    expect(tabKeyOf(tabs![0]!)).toBe("server:alpha");
  });

  it("refuses torn payloads instead of half-believing them", () => {
    expect(sanitizeTabs(undefined)).toBeNull();
    expect(sanitizeTabs([])).toBeNull();
    expect(sanitizeTabs([{}])).toBeNull();
    expect(sanitizeTabs([{ history: [{ kind: "nope" }] }])).toBeNull();
    expect(sanitizeTabs([{ history: [{ kind: "server" }] }])).toBeNull(); // no id
    expect(sanitizeTabs([{ history: [{ kind: "servers" }], historyIndex: 9 }])).toHaveLength(1); // clamped, not rejected
  });
});
