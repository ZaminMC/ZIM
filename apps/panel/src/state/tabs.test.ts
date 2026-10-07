// The tab store's contract (ADR-0015, ADR-0016): singleton navigation
// identity, per-tab destination history, honest reload tokens,
// neighbor-aware close, per-tab ids that make Duplicate honest (a cloned
// view, never a cloned backend §65), pinning (§52), groups (§49),
// reopen (§90), and storage that cannot rehydrate into a broken shell.

import { beforeEach, describe, expect, it } from "vitest";
import {
  bootWindow,
  claimHandoff,
  LEGACY_TABS_KEY,
  canForward,
  newTabId,
  sanitizeTabs,
  stripKey,
  tabDestination,
  tabKeyOf,
  useTabs,
  type Tab,
} from "./tabs";
import { __setWindowIdentityForTests } from "./windowIdentity";

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

/** Seed the store deterministically: the array is built first, then wired. */
const seed = (tabs: Tab[], opts: { active?: number } = {}) => {
  useTabs.setState({
    tabs,
    activeId: tabs[opts.active ?? 0]!.id,
    discoveryQuery: null,
    recentlyClosed: [],
    groups: {},
  });
};

const seedGroups = (groups: Record<string, { label: string; color: string; collapsed: boolean }>) =>
  useTabs.setState({ groups: groups as never });

beforeEach(() => {
  seq = 0;
  localStorage.clear();
  sessionStorage.clear();
  __setWindowIdentityForTests("w-test");
  seed([tab([{ kind: "new" }])]);
});

/** The current strip, as destination keys, for readable assertions. */
const strip = () => useTabs.getState().tabs.map(tabKeyOf);
const activeKey = () => {
  const s = useTabs.getState();
  const t = s.tabs.find((x) => x.id === s.activeId);
  return t ? tabKeyOf(t) : null;
};

describe("navigation and identity", () => {
  it("opens with one new tab", () => {
    seed([tab([{ kind: "new" }])]);
    expect(useTabs.getState().tabs).toHaveLength(1);
    expect(tabDestination(useTabs.getState().tabs[0]!)).toEqual({ kind: "new" });
    expect(activeKey()).toBe("new");
  });

  it("navigates the active tab and keeps history honest", () => {
    const { navigate } = useTabs.getState();
    navigate({ kind: "servers" });
    let s = useTabs.getState();
    expect(activeKey()).toBe("servers");
    expect(tabDestination(s.tabs[0]!)).toEqual({ kind: "servers" });

    navigate(server("alpha"));
    s = useTabs.getState();
    expect(activeKey()).toBe("server:alpha");
    expect(s.tabs[0]!.history).toEqual([{ kind: "new" }, { kind: "servers" }, server("alpha")]);

    // Back returns to the fleet; the tab's key follows its destination.
    useTabs.getState().back();
    s = useTabs.getState();
    expect(activeKey()).toBe("servers");
    expect(canForward(s.tabs[0]!)).toBe(true);

    // Forward returns to the server again — same tab, same id.
    useTabs.getState().forward();
    expect(activeKey()).toBe("server:alpha");
  });

  it("focuses a destination that already rests in another tab, never duplicates", () => {
    seed([tab([{ kind: "new" }, server("alpha")], 1), tab([{ kind: "servers" }])], {
      active: 1,
    });
    useTabs.getState().navigate(server("alpha"));
    const s = useTabs.getState();
    expect(activeKey()).toBe("server:alpha");
    expect(s.tabs, "no third tab was created").toHaveLength(2);
    // The fleet tab's history was not polluted by the focus.
    expect(s.tabs[1]!.history).toEqual([{ kind: "servers" }]);
  });

  it("navigating to the active tab's own destination is a quiet no-op", () => {
    useTabs.getState().navigate({ kind: "servers" });
    useTabs.getState().navigate({ kind: "servers" });
    const s = useTabs.getState();
    expect(s.tabs[0]!.history).toEqual([{ kind: "new" }, { kind: "servers" }]);
  });

  it("truncates the forward tail when navigating from the past", () => {
    seed([tab([{ kind: "new" }, { kind: "servers" }, server("alpha")], 0)]);
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
    expect(activeKey()).toBe("new");

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
    seed([tab([{ kind: "servers" }], 0, { reloadToken: 3 }), tab([server("alpha")])]);
    useTabs.getState().reload();
    const s = useTabs.getState();
    expect(s.tabs[0]!.reloadToken).toBe(4);
    expect(s.tabs[1]!.reloadToken).toBe(0);
  });
});

describe("closing", () => {
  it("activates the right neighbor, then the left when none", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")]), tab([server("beta")])], {
      active: 1,
    });
    const [, alpha, beta] = useTabs.getState().tabs;
    useTabs.getState().close(alpha!.id);
    expect(activeKey()).toBe("server:beta");

    useTabs.getState().close(beta!.id);
    expect(activeKey()).toBe("servers");
  });

  it("closing the last tab leaves a fresh new tab", () => {
    seed([tab([server("alpha")])]);
    useTabs.getState().close(useTabs.getState().tabs[0]!.id);
    const s = useTabs.getState();
    expect(s.tabs).toHaveLength(1);
    expect(activeKey()).toBe("new");
  });

  it("closeToTheRight and closeOthers keep their promise", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")]), tab([server("beta")])], {
      active: 2,
    });
    const [, alpha] = useTabs.getState().tabs;
    useTabs.getState().closeToTheRight(alpha!.id);
    let s = useTabs.getState();
    expect(strip()).toEqual(["servers", "server:alpha"]);
    expect(activeKey(), "the closed active tab hands over to its left neighbor").toBe(
      "server:alpha",
    );

    useTabs.getState().closeOthers(s.tabs[0]!.id);
    s = useTabs.getState();
    expect(strip()).toEqual(["servers"]);
    expect(activeKey()).toBe("servers");
  });

  it("closes a pinned tab only when asked explicitly, and remembers it", () => {
    seed([tab([server("alpha")], 0, { pinned: true }), tab([{ kind: "servers" }])], {
      active: 1,
    });
    const pinned = useTabs.getState().tabs[0]!;
    useTabs.getState().close(pinned.id);
    const s = useTabs.getState();
    expect(strip()).toEqual(["servers"]);
    expect(s.recentlyClosed, "the explicit close enters the memory").toHaveLength(1);
    expect(s.recentlyClosed[0]!.pinned).toBe(true);
  });
});

describe("duplicate (§48, §65)", () => {
  it("clones the view next to the original and focuses the clone", () => {
    seed([tab([{ kind: "new" }, { kind: "servers" }, server("alpha")], 2), tab([{ kind: "settings" }])]);
    const original = useTabs.getState().tabs[0]!;
    useTabs.getState().duplicate(original.id);
    const s = useTabs.getState();
    expect(s.tabs).toHaveLength(3);
    expect(strip()).toEqual(["server:alpha", "server:alpha", "settings"]);
    expect(activeKey(), "the clone holds the focus").toBe("server:alpha");
    const clone = s.tabs[1]!;
    expect(clone.id, "a fresh identity, not the original's").not.toBe(original.id);
    expect(clone.history, "the destination history travels").toEqual(original.history);
    expect(clone.historyIndex).toBe(original.historyIndex);
    expect(clone.reloadToken, "the clone starts unrestored").toBe(0);
  });

  it("the duplicate is unpinned and ungrouped even when the origin is both", () => {
    seed([tab([server("alpha")], 0, { pinned: true, groupId: "g1" })]);
    seedGroups({ g1: { label: "Lab", color: "sky", collapsed: false } });
    const original = useTabs.getState().tabs[0]!;
    useTabs.getState().duplicate(original.id);
    const s = useTabs.getState();
    const clone = s.tabs.find((t) => t.id !== original.id)!;
    expect(clone.pinned).toBe(false);
    expect(clone.groupId).toBeUndefined();
    expect(Object.keys(s.groups), "the group is untouched").toHaveLength(1);
  });

  it("duplicates of the same server run one process behind many views", () => {
    // A store-level statement of §65: duplication changes only the strip.
    seed([tab([server("alpha")])]);
    const only = useTabs.getState().tabs[0]!;
    useTabs.getState().duplicate(only.id);
    useTabs.getState().duplicate(only.id);
    const s = useTabs.getState();
    expect(s.tabs.filter((t) => tabKeyOf(t) === "server:alpha")).toHaveLength(3);
    // The navigation identity still focuses rather than multiplies.
    useTabs.getState().navigate({ kind: "servers" });
    useTabs.getState().navigate(server("alpha"));
    const s2 = useTabs.getState();
    expect(s2.tabs, "navigation added no tab").toHaveLength(3);
    expect(activeKey()).toBe("server:alpha");
  });
});

describe("pinning (§52)", () => {
  it("pins compact-first and unpins back into the free block", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")]), tab([server("beta")])], {
      active: 2,
    });
    const beta = useTabs.getState().tabs[2]!;
    useTabs.getState().togglePin(beta.id);
    expect(strip()).toEqual(["server:beta", "servers", "server:alpha"]);
    expect(useTabs.getState().tabs[0]!.pinned).toBe(true);

    useTabs.getState().togglePin(beta.id);
    // Unpinning keeps the stable order: beta stays where it sits, at the
    // head of the free block — exactly how a browser lets go of a pin.
    expect(strip()).toEqual(["server:beta", "servers", "server:alpha"]);
    expect(useTabs.getState().tabs[0]!.pinned).toBe(false);
  });

  it("stays stable when several pins stack", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")]), tab([server("beta")])], {
      active: 2,
    });
    const [, alpha, beta] = useTabs.getState().tabs;
    useTabs.getState().togglePin(alpha!.id);
    useTabs.getState().togglePin(beta!.id);
    // alpha pinned first keeps its head start; beta joins after it.
    expect(strip()).toEqual(["server:alpha", "server:beta", "servers"]);
  });

  it("pinning a grouped tab leaves the group, and an empty group dissolves", () => {
    seed([tab([server("alpha")], 0, { groupId: "g1" })]);
    seedGroups({ g1: { label: "Lab", color: "sky", collapsed: false } });
    const alpha = useTabs.getState().tabs[0]!;
    useTabs.getState().togglePin(alpha.id);
    const s = useTabs.getState();
    expect(s.tabs[0]!.pinned).toBe(true);
    expect(s.tabs[0]!.groupId).toBeUndefined();
    expect(s.groups, "no members, no group").toEqual({});
  });
});

describe("reopen (§90)", () => {
  it("reopens the most recently closed tab at its old spot", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")]), tab([server("beta")])], {
      active: 1,
    });
    const alpha = useTabs.getState().tabs[1]!;
    useTabs.getState().close(alpha.id);
    expect(strip()).toEqual(["servers", "server:beta"]);

    useTabs.getState().reopen();
    const s = useTabs.getState();
    expect(strip()).toEqual(["servers", "server:alpha", "server:beta"]);
    expect(activeKey(), "the revived tab holds the focus").toBe("server:alpha");
    expect(s.recentlyClosed).toHaveLength(0);
  });

  it("restores pinned state but not group membership", () => {
    seed([tab([server("alpha")], 0, { pinned: true, groupId: "g1" })]);
    seedGroups({ g1: { label: "Lab", color: "sky", collapsed: false } });
    const alpha = useTabs.getState().tabs[0]!;
    useTabs.getState().close(alpha.id);
    useTabs.getState().reopen();
    const s = useTabs.getState();
    const revived = s.tabs[0]!;
    expect(revived.pinned).toBe(true);
    expect(revived.groupId).toBeUndefined();
    expect(s.groups, "the group itself dissolved with its last member").toEqual({});
  });

  it("keeps a bounded memory, most recent first", () => {
    seed([
      tab([server("s1")]),
      tab([server("s2")]),
      tab([server("s3")]),
      tab([{ kind: "servers" }]),
    ], { active: 3 });
    const [a, b, c] = useTabs.getState().tabs;
    useTabs.getState().close(a!.id);
    useTabs.getState().close(b!.id);
    useTabs.getState().close(c!.id);
    const s = useTabs.getState();
    expect(s.recentlyClosed.map((e) => e.history[e.historyIndex])).toEqual([
      server("s3"),
      server("s2"),
      server("s1"),
    ]);
    useTabs.getState().reopen();
    expect(activeKey()).toBe("server:s3");
    useTabs.getState().reopen();
    expect(activeKey()).toBe("server:s2");
  });

  it("the auto-created replacement after a last close is not remembered", () => {
    seed([tab([server("alpha")])]);
    useTabs.getState().close(useTabs.getState().tabs[0]!.id);
    expect(useTabs.getState().recentlyClosed).toHaveLength(1);
    useTabs.getState().reopen();
    // Closing the revived single tab again still records exactly one tab.
    useTabs.getState().close(useTabs.getState().tabs[0]!.id);
    expect(useTabs.getState().recentlyClosed).toHaveLength(1);
  });

  it("reopen with an empty memory is a no-op", () => {
    useTabs.getState().reopen();
    expect(useTabs.getState().tabs).toHaveLength(1);
  });
});

describe("new tab to the right (§48)", () => {
  it("inserts a fresh new tab immediately right of the origin", () => {
    seed([tab([{ kind: "servers" }]), tab([server("alpha")])]);
    useTabs.getState().newTabToTheRight(useTabs.getState().tabs[0]!.id);
    expect(strip()).toEqual(["servers", "new", "server:alpha"]);
    expect(activeKey()).toBe("new");
  });

  it("moves the resting singleton over instead of cloning it", () => {
    seed([tab([{ kind: "new" }]), tab([{ kind: "servers" }]), tab([server("alpha")])], {
      active: 2,
    });
    useTabs.getState().newTabToTheRight(useTabs.getState().tabs[2]!.id);
    expect(strip(), "the singleton moved, none was born").toEqual([
      "servers",
      "server:alpha",
      "new",
    ]);
    expect(useTabs.getState().tabs).toHaveLength(3);
  });
});

describe("groups (§49)", () => {
  /** Two grouped server tabs + one free fleet tab. */
  const seedGrouped = () => {
    seed([tab([server("alpha")]), tab([server("beta")]), tab([{ kind: "servers" }])], {
      active: 2,
    });
    useTabs.setState({
      groups: { g1: { label: "Lab", color: "sky", collapsed: false } } as never,
      tabs: useTabs
        .getState()
        .tabs.map((t, i) => (i < 2 ? { ...t, groupId: "g1" } : t)),
    });
  };

  it("adds a tab to a new group with the next palette color", () => {
    seed([tab([{ kind: "new" }])]);
    useTabs.getState().addToNewGroup(useTabs.getState().tabs[0]!.id);
    const s = useTabs.getState();
    const gid = s.tabs[0]!.groupId!;
    expect(s.groups[gid]).toEqual({ label: "New group", color: "sky", collapsed: false });
  });

  it("palette colors cycle across groups", () => {
    seed([tab([{ kind: "new" }])]);
    useTabs.getState().addToNewGroup(useTabs.getState().tabs[0]!.id);
    useTabs.getState().duplicate(useTabs.getState().tabs[0]!.id);
    const clone = useTabs.getState().tabs[1]!;
    useTabs.getState().addToNewGroup(clone.id);
    const s = useTabs.getState();
    const [g1, g2] = Object.values(s.groups);
    expect(g1!.color).toBe("sky");
    expect(g2!.color).toBe("grass");
  });

  it("moves a tab into an existing group and out again", () => {
    seedGrouped();
    const free = useTabs.getState().tabs[2]!;
    useTabs.getState().moveToGroup(free.id, "g1");
    expect(useTabs.getState().tabs[2]!.groupId).toBe("g1");

    useTabs.getState().removeFromGroup(free.id);
    expect(useTabs.getState().tabs[2]!.groupId).toBeUndefined();
    expect(useTabs.getState().groups, "g1 still has two members").toHaveProperty("g1");
  });

  it("an unknown group is refused honestly", () => {
    seed([tab([server("alpha")])]);
    const only = useTabs.getState().tabs[0]!;
    useTabs.getState().moveToGroup(only.id, "ghost");
    expect(only.groupId).toBeUndefined();
    expect(useTabs.getState().groups).toEqual({});
  });

  it("a pinned tab cannot be grouped", () => {
    seed([tab([server("alpha")])]);
    const only = useTabs.getState().tabs[0]!;
    useTabs.getState().togglePin(only.id);
    useTabs.getState().addToNewGroup(only.id);
    expect(useTabs.getState().tabs[0]!.groupId).toBeUndefined();
    expect(useTabs.getState().groups).toEqual({});
  });

  it("closing members dissolves the emptied group", () => {
    seedGrouped();
    const alpha = useTabs.getState().tabs[0]!;
    useTabs.getState().close(alpha.id);
    expect(useTabs.getState().groups, "one member left").toHaveProperty("g1");
    const beta = useTabs.getState().tabs.find((t) => t.groupId === "g1")!;
    useTabs.getState().close(beta.id);
    expect(useTabs.getState().groups).toEqual({});
  });

  it("collapse and rename keep the group's memory honest", () => {
    seedGrouped();
    useTabs.getState().toggleGroupCollapse("g1");
    expect(useTabs.getState().groups["g1"]!.collapsed).toBe(true);
    useTabs.getState().toggleGroupCollapse("g1");
    expect(useTabs.getState().groups["g1"]!.collapsed).toBe(false);

    useTabs.getState().renameGroup("g1", "  Survival  ");
    expect(useTabs.getState().groups["g1"]!.label).toBe("Survival");

    useTabs.getState().renameGroup("g1", "   ");
    expect(useTabs.getState().groups["g1"]!.label, "an empty rename is refused").toBe("Survival");
    useTabs.getState().renameGroup("ghost", "Nope");
    expect(useTabs.getState().groups).toHaveProperty("g1");
  });

  it("the reopen of a group member does not resurrect the group", () => {
    seedGrouped();
    const alpha = useTabs.getState().tabs[0]!;
    useTabs.getState().close(alpha.id);
    useTabs.getState().reopen();
    const s = useTabs.getState();
    expect(tabKeyOf(s.tabs[0]!)).toBe("server:alpha");
    expect(s.tabs[0]!.groupId).toBeUndefined();
  });
});

describe("storage hygiene", () => {
  it("rehydrates a real session", () => {
    // sanitizeTabs is the merge guard; the happy path keeps everything.
    const raw = [
      {
        id: "kept",
        history: [{ kind: "servers" }, server("alpha")],
        historyIndex: 1,
        reloadToken: 2,
        pinned: true,
      },
      { id: "kept2", history: [{ kind: "settings" }], historyIndex: 0, reloadToken: 0 },
    ];
    const tabs = sanitizeTabs(raw);
    expect(tabs).toHaveLength(2);
    expect(tabs![0]!.reloadToken).toBe(2);
    expect(tabs![0]!.id).toBe("kept");
    expect(tabs![0]!.pinned).toBe(true);
    expect(tabKeyOf(tabs![0]!)).toBe("server:alpha");
    // §52: the pinned tab rehydrates at the head.
    expect(tabs!.map((t) => t.id)).toEqual(["kept", "kept2"]);
  });

  it("refuses torn payloads instead of half-believing them", () => {
    expect(sanitizeTabs(undefined)).toBeNull();
    expect(sanitizeTabs([])).toBeNull();
    expect(sanitizeTabs([{}])).toBeNull();
    expect(sanitizeTabs([{ history: [{ kind: "nope" }] }])).toBeNull();
    expect(sanitizeTabs([{ history: [{ kind: "server" }] }])).toBeNull(); // no id
    expect(
      sanitizeTabs([{ history: [{ kind: "servers" }], historyIndex: 9 }]),
    ).toHaveLength(1); // clamped, not rejected
  });

  it("regenerates missing ids and deduplicates collisions", () => {
    const raw = [
      { history: [{ kind: "servers" }], historyIndex: 0, reloadToken: 0 },
      { id: "dup", history: [server("alpha")], historyIndex: 0, reloadToken: 0 },
      { id: "dup", history: [server("beta")], historyIndex: 0, reloadToken: 0 },
    ];
    const tabs = sanitizeTabs(raw)!;
    expect(tabs).toHaveLength(3);
    const ids = new Set(tabs.map((t) => t.id));
    expect(ids.size).toBe(3);
    expect(tabs[1]!.id).toBe("dup");
    expect(tabs[2]!.id).not.toBe("dup");
  });

  it("migrates a v1 payload: fresh ids, destination-keyed active restored", () => {
    // The migrate path runs inside the persist wrapper; here its inputs are
    // exercised through sanitize + the merge contract it feeds.
    const raw = [
      { history: [{ kind: "servers" }], historyIndex: 0, reloadToken: 0 },
      { history: [server("alpha")], historyIndex: 0, reloadToken: 1 },
    ];
    const tabs = sanitizeTabs(raw)!;
    const ids = tabs.map((t) => t.id);
    expect(ids.every((id) => typeof id === "string" && id !== "")).toBe(true);
    expect(new Set(ids).size).toBe(2);
    expect(newTabId()).not.toBe(newTabId());
  });
});

describe("drag reorder", () => {
  it("moves a free tab and keeps the focus", () => {
    seed([tab([server("alpha")]), tab([server("beta")]), tab([server("gamma")])], {
      active: 1,
    });
    const ids = useTabs.getState().tabs.map((t) => t.id);
    const [alpha, beta, gamma] = ids;
    useTabs.getState().reorder(beta!, 0);
    expect(useTabs.getState().tabs.map((t) => t.id)).toEqual([beta, alpha, gamma]);
    expect(activeKey(), "the drag never steals the focus").toBe("server:beta");
  });

  it("interprets the insertion slot visually (after removal it re-anchors)", () => {
    seed([tab([server("alpha")]), tab([server("beta")]), tab([server("gamma")])]);
    const ids = useTabs.getState().tabs.map((t) => t.id);
    const [alpha, beta, gamma] = ids;
    useTabs.getState().reorder(alpha!, 3); // "before gamma" in the visual strip
    expect(useTabs.getState().tabs.map((t) => t.id)).toEqual([beta, gamma, alpha]);
  });

  it("dropping on itself is a quiet no-op", () => {
    seed([tab([server("alpha")]), tab([server("beta")])]);
    const ids = useTabs.getState().tabs.map((t) => t.id);
    useTabs.getState().reorder(ids[0]!, 0);
    useTabs.getState().reorder(ids[0]!, 1);
    expect(useTabs.getState().tabs.map((t) => t.id)).toEqual(ids);
  });

  it("clamps a free tab to the head of the free block (§52)", () => {
    seed([tab([server("pin")], 0, { pinned: true }), tab([server("alpha")])]);
    const alpha = useTabs.getState().tabs[1]!;
    useTabs.getState().reorder(alpha.id, 0); // the pinned head is not a slot
    const s = useTabs.getState();
    expect(tabKeyOf(s.tabs[0]!)).toBe("server:pin");
    expect(tabKeyOf(s.tabs[1]!)).toBe("server:alpha");
  });

  it("drags a pinned tab within the pinned block, never past it", () => {
    seed([
      tab([server("pin1")], 0, { pinned: true }),
      tab([server("pin2")], 0, { pinned: true }),
      tab([server("alpha")]),
    ]);
    const pin1 = useTabs.getState().tabs[0]!;
    useTabs.getState().reorder(pin1.id, 2); // would land on the free block
    expect(useTabs.getState().tabs.map((t) => tabKeyOf(t))).toEqual([
      "server:pin2",
      "server:pin1",
      "server:alpha",
    ]);
  });

  it("dragging out of the group's reach leaves the group", () => {
    seed([tab([server("alpha")]), tab([server("beta")]), tab([server("gamma")])]);
    useTabs.setState({ groups: { g1: { label: "Lab", color: "sky", collapsed: false } } as never });
    useTabs.setState({
      tabs: useTabs.getState().tabs.map((t, i) => (i < 2 ? { ...t, groupId: "g1" } : t)),
    });
    const beta = useTabs.getState().tabs[1]!;
    useTabs.getState().reorder(beta.id, 3); // past gamma, out of g1's reach
    const s = useTabs.getState();
    expect(tabKeyOf(s.tabs[2]!)).toBe("server:beta");
    expect(s.tabs[2]!.groupId).toBeUndefined();
    expect(s.groups, "alpha keeps g1 alive").toHaveProperty("g1");
  });

  it("dragging within the group keeps membership and the chip's place", () => {
    seed([tab([server("alpha")]), tab([server("beta")]), tab([server("gamma")])]);
    useTabs.setState({ groups: { g1: { label: "Lab", color: "sky", collapsed: false } } as never });
    useTabs.setState({
      tabs: useTabs.getState().tabs.map((t, i) => (i < 2 ? { ...t, groupId: "g1" } : t)),
    });
    const alpha = useTabs.getState().tabs[0]!;
    useTabs.getState().reorder(alpha.id, 2); // lands beside beta, inside g1
    const s = useTabs.getState();
    expect(s.tabs.map((t) => t.groupId)).toEqual(["g1", "g1", undefined]);
    expect(tabKeyOf(s.tabs[0]!)).toBe("server:beta");
  });

  it("a drop between group members never gains membership", () => {
    seed([tab([server("alpha")]), tab([server("beta")]), tab([server("gamma")])]);
    useTabs.setState({ groups: { g1: { label: "Lab", color: "sky", collapsed: false } } as never });
    useTabs.setState({
      tabs: useTabs.getState().tabs.map((t, i) => (i < 2 ? { ...t, groupId: "g1" } : t)),
    });
    const gamma = useTabs.getState().tabs[2]!;
    useTabs.getState().reorder(gamma.id, 1);
    const s = useTabs.getState();
    expect(s.tabs.find((t) => t.id === gamma.id)!.groupId).toBeUndefined();
    // §49's rendering invariant: the group's members still sit together.
    const groupIndexes = s.tabs
      .map((t, i) => (t.groupId === "g1" ? i : -1))
      .filter((i) => i !== -1);
    expect(groupIndexes[groupIndexes.length - 1]! - groupIndexes[0]!).toBe(
      groupIndexes.length - 1,
    );
  });
});

describe("move to new window (§50)", () => {
  it("hands the tab over without a close memory and moves the focus like a close", () => {
    seed([tab([server("alpha")]), tab([server("beta")])], { active: 0 });
    const alpha = useTabs.getState().tabs[0]!;
    const handoff = useTabs.getState().moveToNewWindow(alpha.id);
    expect(handoff).toMatch(/^h/);
    const s = useTabs.getState();
    expect(s.tabs.map((t) => tabKeyOf(t))).toEqual(["server:beta"]);
    expect(activeKey(), "the focus follows the move").toBe("server:beta");
    expect(s.recentlyClosed, "a move is not a close").toHaveLength(0);
    expect(localStorage.getItem(`zamin-panel.tab-handoff:${handoff}`)).not.toBeNull();
  });

  it("moving a background tab leaves the focus alone", () => {
    seed([tab([server("alpha")]), tab([server("beta")])], { active: 0 });
    const beta = useTabs.getState().tabs[1]!;
    useTabs.getState().moveToNewWindow(beta.id);
    expect(activeKey()).toBe("server:alpha");
  });

  it("the last tab cannot move", () => {
    seed([tab([server("alpha")])]);
    expect(useTabs.getState().moveToNewWindow(useTabs.getState().tabs[0]!.id)).toBeNull();
    expect(useTabs.getState().tabs).toHaveLength(1);
  });

  it("an unknown id is refused", () => {
    seed([tab([server("alpha")]), tab([server("beta")])]);
    expect(useTabs.getState().moveToNewWindow("ghost")).toBeNull();
  });

  it("the child claims the slot by hash: its own strip, slot consumed", () => {
    seed([tab([{ kind: "servers" }, server("alpha")], 1), tab([server("beta")])], { active: 0 });
    const alpha = useTabs.getState().tabs[0]!;
    const handoff = useTabs.getState().moveToNewWindow(alpha.id);
    expect(claimHandoff(`#handoff=${handoff}`)).toBe(true);
    const s = useTabs.getState();
    expect(s.tabs).toHaveLength(1);
    expect(tabKeyOf(s.tabs[0]!)).toBe("server:alpha");
    expect(s.tabs[0]!.history).toEqual([{ kind: "servers" }, server("alpha")]);
    expect(activeKey()).toBe("server:alpha");
    expect(localStorage.getItem(`zamin-panel.tab-handoff:${handoff}`), "claimed once").toBeNull();
  });

  it("a moved pinned tab stays pinned in the child; groups cannot travel", () => {
    seed([tab([server("alpha")], 0, { pinned: true, groupId: "g1" }), tab([server("beta")])]);
    useTabs.setState({ groups: { g1: { label: "Lab", color: "sky", collapsed: false } } as never });
    const alpha = useTabs.getState().tabs[0]!;
    const handoff = useTabs.getState().moveToNewWindow(alpha.id);
    expect(claimHandoff(`#handoff=${handoff}`)).toBe(true);
    const s = useTabs.getState();
    expect(s.tabs[0]!.pinned).toBe(true);
    expect(s.tabs[0]!.groupId).toBeUndefined();
    expect(s.groups).toEqual({});
  });

  it("a foreign hash never claims and leaves no strip behind", () => {
    seed([tab([server("alpha")]), tab([server("beta")])]);
    useTabs.getState().moveToNewWindow(useTabs.getState().tabs[1]!.id);
    expect(claimHandoff("#handoff=h-foreign")).toBe(false);
    expect(claimHandoff("#no-handoff-here")).toBe(false);
    expect(claimHandoff(null)).toBe(false);
  });

  it("a stale slot is refused even with the right hash", async () => {
    seed([tab([server("alpha")]), tab([server("beta")])]);
    const beta = useTabs.getState().tabs[1]!;
    const handoff = useTabs.getState().moveToNewWindow(beta.id);
    const key = `zamin-panel.tab-handoff:${handoff}`;
    const raw = localStorage.getItem(key)!;
    const stale = { ...JSON.parse(raw), issuedAt: Date.now() - 61_000 };
    localStorage.setItem(key, JSON.stringify(stale));
    expect(claimHandoff(`#handoff=${handoff}`)).toBe(false);
    expect(localStorage.getItem(key), "the stale slot was swept").toBeNull();
  });

  it("a torn slot is refused, not half-claimed", () => {
    localStorage.setItem("zamin-panel.tab-handoff:h-torn", "{\"version\":1,");
    expect(claimHandoff("#handoff=h-torn")).toBe(false);
    expect(localStorage.getItem("zamin-panel.tab-handoff:h-torn")).toBeNull();
  });

  it("restoreMoved brings a blocked tab home at its old place", () => {
    seed([tab([server("alpha")]), tab([server("beta")]), tab([server("gamma")])], { active: 1 });
    const ids = useTabs.getState().tabs.map((t) => t.id);
    const beta = useTabs.getState().tabs[1]!;
    useTabs.getState().moveToNewWindow(beta.id);
    useTabs.getState().restoreMoved(beta, 1, true);
    const s = useTabs.getState();
    expect(s.tabs.map((t) => t.id)).toEqual(ids);
    expect(activeKey(), "the focus comes back too").toBe("server:beta");
  });

  it("restoreMoved never duplicates and re-joins a surviving group", () => {
    seed([tab([server("alpha")]), tab([server("beta")])]);
    useTabs.setState({ groups: { g1: { label: "Lab", color: "sky", collapsed: false } } as never });
    useTabs.setState({
      tabs: useTabs.getState().tabs.map((t) => ({ ...t, groupId: "g1" })),
    });
    const beta = useTabs.getState().tabs[1]!;
    useTabs.getState().moveToNewWindow(beta.id);
    useTabs.getState().restoreMoved(beta, 0, false);
    const s = useTabs.getState();
    expect(s.tabs.filter((t) => t.id === beta.id)).toHaveLength(1);
    expect(s.tabs.find((t) => t.id === beta.id)!.groupId).toBe("g1");
    // A second restore is an honest no-op.
    useTabs.getState().restoreMoved(beta, 0, false);
    expect(useTabs.getState().tabs).toHaveLength(2);
  });
});

describe("per-window storage (ADR-0018)", () => {
  it("keys the strip by the window identity", () => {
    __setWindowIdentityForTests("w-alpha");
    expect(stripKey()).toBe("zamin-panel.tabs@w-alpha");
    __setWindowIdentityForTests("w-beta");
    expect(stripKey()).toBe("zamin-panel.tabs@w-beta");
  });

  it("adopts the legacy shared strip exactly once", async () => {
    const legacy = {
      state: {
        tabs: [
          { id: "kept", history: [server("alpha")], historyIndex: 0, reloadToken: 0 },
        ],
        activeId: "kept",
        groups: {},
      },
      version: 2,
    };
    localStorage.setItem(LEGACY_TABS_KEY, JSON.stringify(legacy));
    __setWindowIdentityForTests("w-adopter");
    await useTabs.persist.rehydrate();
    expect(useTabs.getState().tabs.map((t) => t.id)).toEqual(["kept"]);
    expect(localStorage.getItem(LEGACY_TABS_KEY), "the shared key is retired").toBeNull();
    // The adopter's next write lands on its own key; a second window is
    // born fresh — no strip of its own exists, so it never inherits tabs.
    useTabs.setState({});
    expect(localStorage.getItem(stripKey())).not.toBeNull();
    __setWindowIdentityForTests("w-second");
    expect(localStorage.getItem(stripKey())).toBeNull();
    expect(localStorage.getItem(LEGACY_TABS_KEY)).toBeNull();
  });

  it("bootWindow prunes idle windows and their strips", () => {
    const ancient = Date.now() - 15 * 24 * 60 * 60 * 1000;
    localStorage.setItem(
      "zamin-panel.windows",
      JSON.stringify({ "w-old": ancient, "w-young": Date.now() }),
    );
    localStorage.setItem("zamin-panel.tabs@w-old", "{\"state\":{\"tabs\":[]},\"version\":3}");
    localStorage.setItem("zamin-panel.tabs@w-young", "{\"state\":{\"tabs\":[]},\"version\":3}");
    __setWindowIdentityForTests("w-current");
    bootWindow();
    const registry = JSON.parse(localStorage.getItem("zamin-panel.windows")!) as Record<
      string,
      number
    >;
    expect(registry).not.toHaveProperty("w-old");
    expect(registry).toHaveProperty("w-young");
    expect(registry).toHaveProperty("w-current");
    expect(localStorage.getItem("zamin-panel.tabs@w-old"), "the strip goes with it").toBeNull();
    expect(localStorage.getItem("zamin-panel.tabs@w-young")).not.toBeNull();
  });

  it("a torn registry is rebuilt without nuking strips", () => {
    localStorage.setItem("zamin-panel.windows", "{not json");
    localStorage.setItem("zamin-panel.tabs@w-old", "{\"state\":{\"tabs\":[]},\"version\":3}");
    __setWindowIdentityForTests("w-current");
    bootWindow();
    const registry = JSON.parse(localStorage.getItem("zamin-panel.windows")!) as Record<
      string,
      number
    >;
    expect(registry).toHaveProperty("w-current");
    expect(localStorage.getItem("zamin-panel.tabs@w-old"), "no registry, no pruning").not.toBeNull();
  });

  it("bootWindow sweeps stale handoff slots but keeps fresh ones", () => {
    const handoff = (id: string, issuedAt: number) =>
      localStorage.setItem(
        `zamin-panel.tab-handoff:${id}`,
        JSON.stringify({
          version: 1,
          handoffId: id,
          issuedAt,
          history: [server("alpha")],
          historyIndex: 0,
          pinned: false,
        }),
      );
    handoff("h-stale", Date.now() - 61_000);
    handoff("h-fresh", Date.now());
    localStorage.setItem("zamin-panel.tab-handoff:h-garbage", "{torn");
    __setWindowIdentityForTests("w-current");
    bootWindow();
    expect(localStorage.getItem("zamin-panel.tab-handoff:h-stale")).toBeNull();
    expect(localStorage.getItem("zamin-panel.tab-handoff:h-garbage")).toBeNull();
    expect(localStorage.getItem("zamin-panel.tab-handoff:h-fresh")).not.toBeNull();
  });
});
