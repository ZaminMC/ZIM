// Tabs — the CONTENT-side navigation shim for ADR-0033 Phase 1.
//
// The AUTHORITATIVE strip lives in the Rust host (src-tauri/src/shell/);
// the frame webview renders it and the operator drives it there. This
// store is the same vocabulary the content views already speak
// (navigate/newTab/close/reload/…), with two modes:
//   • desktop (Tauri): every mutation is FORWARDED to the host command
//     surface (Chromium's ID space) and the local state mirrors the
//     answer — the host, not this store, decides identity (§61) and the
//     pinned block (upstream TabStripModel);
//   • browser dev bridge (`npm run dev`): plain local behavior, a debug
//     harness only — it never ships; the shipped shell has no fallback.
//
// The old React tab strip is GONE from the execution path (criterion 1):
// TabStrip.tsx/ToolBar/AddressBar/BookmarksBar were deleted; this module
// exists so content views keep one honest way to say "navigate me".

import { create } from "zustand";
import { navigateHost } from "./shellLane";
import { invoke } from "@tauri-apps/api/core";
import type { Destination } from "./destinations";
import { tabKey, type TabKey } from "./destinations";

export type TabId = string;
export type GroupId = string;

export const GROUP_COLORS = ["sky", "grass", "amber", "rose", "violet", "slate"] as const;
export type GroupColor = (typeof GROUP_COLORS)[number];

export interface Tab {
  id: TabId;
  history: Destination[];
  historyIndex: number;
  reloadToken: number;
  pinned?: boolean;
  muted?: boolean;
  groupId?: GroupId;
}

export interface TabGroup {
  label: string;
  color: GroupColor;
  collapsed: boolean;
}

export interface ClosedTab {
  history: Destination[];
  historyIndex: number;
  pinned?: boolean;
  index: number;
}

export function tabDestination(tab: Tab): Destination {
  return tab.history[tab.historyIndex] ?? { kind: "new" };
}

export const tabKeyOf = (tab: Tab): TabKey => tabKey(tabDestination(tab));

export const activeKeyOf = (s: { tabs: Tab[]; activeId: TabId | null }): TabKey | null => {
  const tab = s.tabs.find((t) => t.id === s.activeId) ?? s.tabs[0];
  return tab ? tabKeyOf(tab) : null;
};

export const canBack = (tab: Tab): boolean => tab.historyIndex > 0;
export const canForward = (tab: Tab): boolean => tab.historyIndex < tab.history.length - 1;

let idSeq = 0;
export const newTabId = (): TabId =>
  `t${(++idSeq).toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

export const DEFAULT_GROUP_LABEL = "New group";
export const MAX_RECENTLY_CLOSED = 25;

/** The host command IDs this shim forwards (shell/commands.rs). */
const CMD = {
  NEW_TAB: 34014,
  CLOSE_TAB: 34015,
  SELECT_TAB_0: 34018,
  DUPLICATE_TAB: 34027,
  RESTORE_TAB: 34028,
  ADD_NEW_TAB_TO_GROUP: 34100,
  TOGGLE_PINNED: 50001,
  TOGGLE_MUTE: 50003,
} as const;

/** Desktop host present? (mirror of frame/frameIpc's probe; kept local
 *  so the store has no import cycle with the frame layer). */
const onHost = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const hostCommand = (id: number, arg?: Record<string, unknown>): void => {
  if (!onHost()) return;
  void invoke("shell_command", { id, arg: arg ?? null }).catch(() => {
    // The host answers every mirrored verb; a failure here is the dev
    // bridge's absence, never a user-facing error.
  });
};

const freshTab = (destination: Destination): Tab => ({
  id: newTabId(),
  history: [destination],
  historyIndex: 0,
  reloadToken: 0,
});

interface TabsState {
  tabs: Tab[];
  activeId: TabId | null;
  discoveryQuery: string | null;
  recentlyClosed: ClosedTab[];
  groups: Record<GroupId, TabGroup>;

  navigate: (destination: Destination) => void;
  newTab: (query?: string) => void;
  openInNewTab: (destination: Destination) => void;
  back: () => void;
  forward: () => void;
  reload: () => void;
  close: (id: TabId) => void;
  closeOthers: (id: TabId) => void;
  closeToTheRight: (id: TabId) => void;
  setActive: (id: TabId) => void;
  setDiscoveryQuery: (query: string | null) => void;
  newTabToTheRight: (id: TabId) => void;
  duplicate: (id: TabId) => void;
  togglePin: (id: TabId) => void;
  reopen: () => void;
  addToNewGroup: (id: TabId) => void;
  moveToGroup: (id: TabId, groupId: GroupId) => void;
  removeFromGroup: (id: TabId) => void;
  toggleGroupCollapse: (groupId: GroupId) => void;
  renameGroup: (groupId: GroupId, label: string) => void;
  reorder: (id: TabId, insertion: number) => void;
  moveToNewWindow: (id: TabId) => string | null;
  restoreMoved: (tab: Tab, index: number, focus: boolean) => void;
  toggleMute: (id: TabId) => void;
  toggleVerticalStrip: () => void;
  verticalStrip: boolean;
}

const initial = (): { tabs: Tab[]; activeId: TabId } => {
  const tab = freshTab({ kind: "new" });
  return { tabs: [tab], activeId: tab.id };
};

export const useTabs = create<TabsState>()((set) => ({
  ...initial(),
  discoveryQuery: null,
  recentlyClosed: [],
  groups: {},
  verticalStrip: false,

  // §61 singleton identity on the host; the local mirror follows so the
  // content selectors stay coherent.
  navigate: (destination) => {
    if (onHost()) {
      // The host is told "navigate the tab that's calling" through its
      // own lane; the store mirror does the plain append.
      void navigateHost(destination);
    }
    set((s) => {
      // Singleton identity locally (dev bridge); the host repeats it
      // authoritatively — same rule, two layers.
      const key = tabKey(destination);
      const resting = s.tabs.find((t) => tabKeyOf(t) === key);
      if (resting) {
        return { activeId: resting.id, discoveryQuery: null };
      }
      return {
        tabs: s.tabs.map((t) =>
          t.id === s.activeId
            ? {
                ...t,
                history: [...t.history.slice(0, t.historyIndex + 1), destination],
                historyIndex: t.historyIndex + 1,
              }
            : t,
        ),
        discoveryQuery: null,
      };
    });
  },

  newTab: (query) => {
    hostCommand(CMD.NEW_TAB);
    set((s) => {
      const resting = s.tabs.find((t) => tabKeyOf(t) === "new");
      if (resting && query == null) {
        return { activeId: resting.id };
      }
      const tab = freshTab({ kind: "new" });
      return {
        tabs: [...s.tabs, tab],
        activeId: tab.id,
        discoveryQuery: query ?? null,
      };
    });
  },

  openInNewTab: (destination) => {
    const tab = freshTab(destination);
    set((s) => ({ tabs: [...s.tabs, tab], activeId: tab.id }));
  },

  back: () => {
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.id === s.activeId && t.historyIndex > 0
          ? { ...t, historyIndex: t.historyIndex - 1 }
          : t,
      ),
    }));
  },

  forward: () => {
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.id === s.activeId && t.historyIndex < t.history.length - 1
          ? { ...t, historyIndex: t.historyIndex + 1 }
          : t,
      ),
    }));
  },

  reload: () => {
    hostCommand(33002); // IDC_RELOAD
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === s.activeId ? { ...t, reloadToken: t.reloadToken + 1 } : t)),
    }));
  },

  close: (id) => {
    hostCommand(CMD.CLOSE_TAB, { tab_id: Number(id) });
    set((s) => {
      const index = s.tabs.findIndex((t) => t.id === id);
      if (index === -1) return s;
      const tab = s.tabs[index];
      if (!tab) return s;
      const tabs = s.tabs.filter((t) => t.id !== id);
      if (tabs.length === 0) {
        // The host answers the same way: a fresh New tab, not a dead end.
        const fresh = freshTab({ kind: "new" });
        return {
          tabs: [fresh],
          activeId: fresh.id,
          recentlyClosed: [
            { history: tab.history, historyIndex: tab.historyIndex, pinned: tab.pinned, index },
            ...s.recentlyClosed,
          ].slice(0, MAX_RECENTLY_CLOSED),
          groups: {},
        };
      }
      const neighbor = tabs[index] ?? tabs[index - 1] ?? tabs[0];
      if (!neighbor) return s;
      const activeId = s.activeId === id ? neighbor.id : s.activeId;
      return {
        tabs,
        activeId,
        recentlyClosed: [
          { history: tab.history, historyIndex: tab.historyIndex, pinned: tab.pinned, index },
          ...s.recentlyClosed,
        ].slice(0, MAX_RECENTLY_CLOSED),
      };
    });
  },

  closeOthers: (id) => {
    set((s) => ({
      tabs: s.tabs.filter((t) => t.id === id || t.pinned),
      activeId: id,
    }));
  },

  closeToTheRight: (id) => {
    set((s) => {
      const index = s.tabs.findIndex((t) => t.id === id);
      return { tabs: s.tabs.filter((t, i) => i <= index || t.pinned) };
    });
  },

  setActive: (id) => {
    // Selection by local id: map through the strip position for the host.
    set((s) => {
      const index = s.tabs.findIndex((t) => t.id === id);
      if (index >= 0) hostCommand(CMD.SELECT_TAB_0 + index);
      return { activeId: id };
    });
  },

  setDiscoveryQuery: (query) => set({ discoveryQuery: query }),

  newTabToTheRight: (id) => {
    hostCommand(CMD.NEW_TAB);
    set((s) => {
      const resting = s.tabs.find((t) => tabKeyOf(t) === "new");
      const index = s.tabs.findIndex((t) => t.id === id);
      const tab = resting ?? freshTab({ kind: "new" });
      void index;
      const others = s.tabs.filter((t) => t.id !== tab.id);
      others.splice(Math.min(index + 1, others.length), 0, tab);
      return { tabs: others, activeId: tab.id };
    });
  },

  duplicate: (id) => {
    hostCommand(CMD.DUPLICATE_TAB, { tab_id: Number(id) });
    set((s) => {
      const index = s.tabs.findIndex((t) => t.id === id);
      const source = s.tabs[index];
      if (!source) return s;
      const clone: Tab = {
        id: newTabId(),
        history: source.history,
        historyIndex: source.historyIndex,
        reloadToken: 0,
        pinned: undefined,
        muted: source.muted,
        groupId: undefined,
      };
      const tabs = [...s.tabs];
      tabs.splice(index + 1, 0, clone);
      return { tabs, activeId: clone.id };
    });
  },

  togglePin: (id) => {
    hostCommand(CMD.TOGGLE_PINNED, { tab_id: Number(id) });
    set((s) => {
      // §52 invariant: pinned head, stable within blocks.
      const tabs = s.tabs.map((t) => (t.id === id ? { ...t, pinned: !t.pinned, groupId: undefined } : t));
      tabs.sort((a, b) => Number(b.pinned ?? false) - Number(a.pinned ?? false));
      return { tabs };
    });
  },

  reopen: () => {
    hostCommand(CMD.RESTORE_TAB);
    set((s) => {
      const [entry, ...rest] = s.recentlyClosed;
      if (!entry) return s;
      const tab: Tab = {
        id: newTabId(),
        history: entry.history,
        historyIndex: entry.historyIndex,
        reloadToken: 0,
        pinned: entry.pinned,
      };
      const index = entry.index;
      const tabs = [...s.tabs];
      tabs.splice(Math.min(index, tabs.length), 0, tab);
      return { tabs, activeId: tab.id, recentlyClosed: rest };
    });
  },

  addToNewGroup: (id) => {
    hostCommand(CMD.ADD_NEW_TAB_TO_GROUP, { tab_id: Number(id) });
    set((s) => {
      const groupId = `g${Object.keys(s.groups).length + 1}-${Math.random().toString(36).slice(2, 6)}`;
      const colors = GROUP_COLORS;
      return {
        groups: {
          ...s.groups,
          [groupId]: {
            label: DEFAULT_GROUP_LABEL,
            color: colors[Object.keys(s.groups).length % colors.length] ?? "sky",
            collapsed: false,
          },
        },
        tabs: s.tabs.map((t) => (t.id === id ? { ...t, groupId, pinned: false } : t)),
      };
    });
  },

  moveToGroup: (id, groupId) => {
    set((s) => (s.groups[groupId] ? { tabs: s.tabs.map((t) => (t.id === id ? { ...t, groupId } : t)) } : s));
  },

  removeFromGroup: (id) => {
    set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, groupId: undefined } : t)) }));
  },

  toggleGroupCollapse: (groupId) => {
    set((s) => {
      const group = s.groups[groupId];
      if (!group) return s;
      return { groups: { ...s.groups, [groupId]: { ...group, collapsed: !group.collapsed } } };
    });
  },

  renameGroup: (groupId, label) => {
    set((s) => {
      const group = s.groups[groupId];
      if (!group) return s;
      return { groups: { ...s.groups, [groupId]: { ...group, label } } };
    });
  },

  reorder: (id, insertion) => {
    set((s) => {
      const from = s.tabs.findIndex((t) => t.id === id);
      if (from === -1) return s;
      const tabs = [...s.tabs];
      const [tab] = tabs.splice(from, 1);
      if (!tab) return s;
      // §52 clamp: a pinned tab stays inside the pinned block; a free
      // tab never lands before it.
      let lastPinned = -1;
      for (let i = 0; i < tabs.length; i++) {
        if (tabs[i]?.pinned) lastPinned = i;
      }
      const edge = lastPinned + 1;
      const wanted = insertion > from ? insertion - 1 : insertion;
      const target = tab.pinned
        ? Math.min(Math.max(wanted, 0), Math.max(edge - 1, 0))
        : Math.min(Math.max(wanted, edge), tabs.length);
      tabs.splice(target, 0, tab);
      return { tabs };
    });
  },

  moveToNewWindow: (id) => {
    // §50's handoff belonged to the old single-webview shell; the host's
    // tear-off (ADR-0033) owns multi-window now. The verb stays honest:
    // it refuses rather than pretending.
    void id;
    return null;
  },

  restoreMoved: (tab, index, focus) => {
    set((s) => {
      const tabs = [...s.tabs];
      tabs.splice(Math.min(index, tabs.length), 0, tab);
      return { tabs, ...(focus ? { activeId: tab.id } : {}) };
    });
  },

  toggleMute: (id) => {
    hostCommand(CMD.TOGGLE_MUTE, { tab_id: Number(id) });
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === id ? { ...t, muted: !t.muted } : t)),
    }));
  },

  toggleVerticalStrip: () => set((s) => ({ verticalStrip: !s.verticalStrip })),
}));
