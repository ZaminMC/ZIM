// Tabs (ADR-0015, ADR-0016): ordered browser tabs of typed destinations,
// per-tab destination history (§59), a reload token (§60), and singleton
// NAVIGATION identity (§61: opening a resting destination focuses it).
// Tab state is PANEL-LOCAL storage, never daemon state.
//
// ADR-0016 adds the operator's tab machinery (§48–52, §90): each tab now
// carries a stable id, so DUPLICATE (§48) can clone the VIEW — a second
// tab, isolated UI state (§51), same destination — without touching the
// one server process behind it (§65: no duplicate backend). Tabs can be
// pinned (§52: compact, first, protected), grouped (§49: collapsible,
// browser-style — not folders), closed into a recent-closed memory and
// reopened (§90). Navigation still never duplicates identity: it focuses
// the resting tab, preferring the active one when it already rests there.
//
// Invariant: pinned tabs always sit at the head of the strip (§52), each
// block in stable order; every mutation re-normalizes.

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { tabKey, type Destination, type TabKey } from "./destinations";

export type TabId = string;
export type GroupId = string;

/** The group color palette — indexes into the strip's CSS group colors. */
export const GROUP_COLORS = ["sky", "grass", "amber", "rose", "violet", "slate"] as const;
export type GroupColor = (typeof GROUP_COLORS)[number];

export interface Tab {
  /** Stable per-tab identity (ADR-0016): survives navigation, drives the
   *  strip/active selection/close/duplicate. NOT the destination key. */
  id: TabId;
  /** Destination history, oldest first; current = history[historyIndex]. */
  history: Destination[];
  historyIndex: number;
  /** §60 reload: bumped on demand; the mounted view keys off it and
   *  rebuilds. Never touches the server process. */
  reloadToken: number;
  /** §52 pinned: compact, at the strip's head, guarded from accidents. */
  pinned?: boolean;
  /** §49 group membership; groups live in the store's `groups` map. */
  groupId?: GroupId;
}

export interface TabGroup {
  label: string;
  color: GroupColor;
  collapsed: boolean;
}

/** A closed tab, remembered for Reopen (§90). The group membership is
 *  deliberately dropped — a reopened tab is a fresh view. */
export interface ClosedTab {
  history: Destination[];
  historyIndex: number;
  pinned?: boolean;
  /** Strip position at close time; reopen clamps it to the strip's size. */
  index: number;
}

/** The tab's current destination, guarded against torn storage. */
export function tabDestination(tab: Tab): Destination {
  return tab.history[tab.historyIndex] ?? { kind: "new" };
}

export const tabKeyOf = (tab: Tab): TabKey => tabKey(tabDestination(tab));

/** The destination key of whichever tab currently holds the focus — the
 *  chrome's shorthand (address bar resting text, palette context, tests). */
export const activeKeyOf = (s: { tabs: Tab[]; activeId: TabId | null }): TabKey | null => {
  const tab = s.tabs.find((t) => t.id === s.activeId) ?? s.tabs[0];
  return tab ? tabKeyOf(tab) : null;
};

export const canBack = (tab: Tab): boolean => tab.historyIndex > 0;
export const canForward = (tab: Tab): boolean => tab.historyIndex < tab.history.length - 1;

let idSeq = 0;
/** A fresh unique tab id (counter + entropy; storage collisions regenerate). */
export const newTabId = (): TabId =>
  `t${(++idSeq).toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

const freshTab = (destination: Destination): Tab => ({
  id: newTabId(),
  history: [destination],
  historyIndex: 0,
  reloadToken: 0,
});

const defaultTabs = (): Tab[] => [freshTab({ kind: "new" })];

const MAX_RECENTLY_CLOSED = 25;
const MAX_GROUP_LABEL = 24;
export const DEFAULT_GROUP_LABEL = "New group";

interface TabsState {
  tabs: Tab[];
  activeId: TabId | null;
  /** The query dialect's landing spot: free text typed in the address bar
   *  opens the discovery page carrying this text. Ephemeral by design —
   *  a search refinement is not a destination change (§59). */
  discoveryQuery: string | null;
  /** §90: closed tabs, most recent first. Session-scoped, not persisted. */
  recentlyClosed: ClosedTab[];
  /** §49: groups by id. A group with no members dissolves. */
  groups: Record<GroupId, TabGroup>;

  navigate: (destination: Destination) => void;
  newTab: (query?: string) => void;
  back: () => void;
  forward: () => void;
  reload: () => void;
  close: (id: TabId) => void;
  closeOthers: (id: TabId) => void;
  closeToTheRight: (id: TabId) => void;
  setActive: (id: TabId) => void;
  setDiscoveryQuery: (query: string | null) => void;
  /** §48: new tab immediately to the right of `id` (the new tab stays a
   *  singleton: a resting one is moved over, not cloned). */
  newTabToTheRight: (id: TabId) => void;
  /** §48/§65: duplicate the VIEW — fresh id, same destination history,
   *  isolated state; never a second backend. Lands unpinned, ungrouped. */
  duplicate: (id: TabId) => void;
  /** §52. Pinning a grouped tab leaves the group. */
  togglePin: (id: TabId) => void;
  /** §90: reopen the most recently closed tab at (clamped) old position. */
  reopen: () => void;
  /** §49: group machinery. Refuses pinned tabs honestly. */
  addToNewGroup: (id: TabId) => void;
  moveToGroup: (id: TabId, groupId: GroupId) => void;
  removeFromGroup: (id: TabId) => void;
  toggleGroupCollapse: (groupId: GroupId) => void;
  renameGroup: (groupId: GroupId, label: string) => void;
}

/** The active tab, or the first tab when the stored id went stale. */
function activeOf(tabs: Tab[], activeId: TabId | null): Tab | undefined {
  return tabs.find((t) => t.id === activeId) ?? tabs[0];
}

/** §52 invariant: pinned tabs at the head, each block in stable order. */
function normalizeOrder(tabs: Tab[]): Tab[] {
  const pinned = tabs.filter((t) => t.pinned);
  const rest = tabs.filter((t) => !t.pinned);
  return [...pinned, ...rest];
}

/** §49 rendering invariant: a group's members sit together in the strip,
 *  so its chip has one place to live. Joining a group moves the tab next
 *  to its group (the strip is presentation; identity never moves). */
function groupAdjacent(tabs: Tab[]): Tab[] {
  const out: Tab[] = [];
  const lastIndex = new Map<GroupId, number>();
  for (const t of tabs) {
    const gid = t.groupId;
    const seen = gid !== undefined ? lastIndex.get(gid) : undefined;
    if (gid !== undefined && seen !== undefined) {
      const at = seen + 1;
      out.splice(at, 0, t);
      lastIndex.set(gid, at);
    } else {
      out.push(t);
      if (gid !== undefined) lastIndex.set(gid, out.length - 1);
    }
  }
  return out;
}

function normalize(tabs: Tab[]): Tab[] {
  return groupAdjacent(normalizeOrder(tabs));
}

/** Travel within one tab: truncate the forward tail, append the destination. */
function travel(tab: Tab, destination: Destination): Tab {
  const history = [...tab.history.slice(0, tab.historyIndex + 1), destination];
  return { ...tab, history, historyIndex: history.length - 1 };
}

/** Dissolve groups left without members; drop memberships without groups. */
function tidyGroups(tabs: Tab[], groups: Record<GroupId, TabGroup>): Record<GroupId, TabGroup> {
  const surviving: Record<GroupId, TabGroup> = {};
  for (const tab of tabs) {
    const gid = tab.groupId;
    const group = gid !== undefined ? groups[gid] : undefined;
    if (gid !== undefined && group) surviving[gid] = group;
  }
  for (const tab of tabs) {
    if (tab.groupId && !surviving[tab.groupId]) delete tab.groupId;
  }
  return surviving;
}

function rememberClosed(recentlyClosed: ClosedTab[], tab: Tab, index: number): ClosedTab[] {
  const entry: ClosedTab = {
    history: tab.history,
    historyIndex: tab.historyIndex,
    pinned: tab.pinned,
    index,
  };
  return [entry, ...recentlyClosed].slice(0, MAX_RECENTLY_CLOSED);
}

// --- storage hygiene ----------------------------------------------------------
// localStorage is untrusted input: a torn or foreign payload rehydrates as
// the default session (one new tab), never as a broken shell.

function sanitizeDestination(value: unknown): Destination | null {
  if (!value || typeof value !== "object") return null;
  const v = value as Record<string, unknown>;
  switch (v.kind) {
    case "servers":
    case "new":
    case "settings":
      return { kind: v.kind };
    case "server":
      return typeof v.serverId === "string" && v.serverId !== ""
        ? { kind: "server", serverId: v.serverId }
        : null;
    case "missing":
      return typeof v.url === "string" && v.url !== "" ? { kind: "missing", url: v.url } : null;
    default:
      return null;
  }
}

export function sanitizeTabs(raw: unknown): Tab[] | null {
  if (!Array.isArray(raw) || raw.length === 0) return null;
  const tabs: Tab[] = [];
  const seen = new Set<TabId>();
  for (const entry of raw) {
    if (!entry || typeof entry !== "object") return null;
    const v = entry as Record<string, unknown>;
    if (!Array.isArray(v.history) || v.history.length === 0) return null;
    const history: Destination[] = [];
    for (const d of v.history) {
      const dest = sanitizeDestination(d);
      if (!dest) return null;
      history.push(dest);
    }
    const index =
      typeof v.historyIndex === "number" &&
      Number.isInteger(v.historyIndex) &&
      v.historyIndex >= 0 &&
      v.historyIndex < history.length
        ? v.historyIndex
        : history.length - 1;
    const token =
      typeof v.reloadToken === "number" && Number.isFinite(v.reloadToken) && v.reloadToken >= 0
        ? Math.floor(v.reloadToken)
        : 0;
    // Ids: keep stored ones (deduplicated); torn payloads get fresh ids.
    let id = typeof v.id === "string" && v.id !== "" ? v.id : newTabId();
    if (seen.has(id)) id = newTabId();
    seen.add(id);
    const pinned = v.pinned === true;
    const groupId = typeof v.groupId === "string" && v.groupId !== "" ? v.groupId : undefined;
    tabs.push({ id, history, historyIndex: index, reloadToken: token, pinned, groupId });
  }
  return normalize(tabs);
}

function sanitizeGroups(raw: unknown): Record<GroupId, TabGroup> {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return {};
  const out: Record<GroupId, TabGroup> = {};
  for (const [id, value] of Object.entries(raw as Record<string, unknown>)) {
    if (!value || typeof value !== "object") continue;
    const v = value as Record<string, unknown>;
    const label = typeof v.label === "string" ? v.label.slice(0, MAX_GROUP_LABEL) : "";
    const color = GROUP_COLORS.includes(v.color as GroupColor) ? (v.color as GroupColor) : "slate";
    out[id] = { label: label || DEFAULT_GROUP_LABEL, color, collapsed: v.collapsed === true };
  }
  return out;
}

export const useTabs = create<TabsState>()(
  persist(
    (set) => ({
      tabs: defaultTabs(),
      activeId: null,
      discoveryQuery: null,
      recentlyClosed: [],
      groups: {},

      navigate: (destination) =>
        set((state) => {
          const key = tabKey(destination);
          // Identity discipline (§61): already resting somewhere? Focus it.
          // The active tab rests here already: nothing travels, nothing moves.
          const current = activeOf(state.tabs, state.activeId);
          if (current && tabKeyOf(current) === key) {
            return { activeId: current.id, discoveryQuery: null };
          }
          const resting = state.tabs.find((t) => tabKeyOf(t) === key);
          if (resting) return { activeId: resting.id, discoveryQuery: null };
          if (!current) {
            const fresh = freshTab(destination);
            return {
              tabs: [fresh],
              activeId: fresh.id,
              groups: {},
              discoveryQuery: null,
            };
          }
          const from = current.id;
          const tabs = state.tabs.map((t) => (t.id === from ? travel(t, destination) : t));
          // The tab keeps its identity; its derived key follows the destination.
          return { tabs, activeId: from, discoveryQuery: null };
        }),

      newTab: (query) =>
        set((state) => {
          const resting = state.tabs.find((t) => tabKeyOf(t) === "new");
          if (resting) {
            return { tabs: state.tabs, activeId: resting.id, discoveryQuery: query ?? null };
          }
          const fresh = freshTab({ kind: "new" });
          return {
            tabs: [...state.tabs, fresh],
            activeId: fresh.id,
            discoveryQuery: query ?? null,
          };
        }),

      back: () =>
        set((state) => {
          const current = activeOf(state.tabs, state.activeId);
          if (!current || !canBack(current)) return state;
          const moved = { ...current, historyIndex: current.historyIndex - 1 };
          return {
            tabs: state.tabs.map((t) => (t.id === current.id ? moved : t)),
            activeId: moved.id,
          };
        }),

      forward: () =>
        set((state) => {
          const current = activeOf(state.tabs, state.activeId);
          if (!current || !canForward(current)) return state;
          const moved = { ...current, historyIndex: current.historyIndex + 1 };
          return {
            tabs: state.tabs.map((t) => (t.id === current.id ? moved : t)),
            activeId: moved.id,
          };
        }),

      reload: () =>
        set((state) => {
          const current = activeOf(state.tabs, state.activeId);
          if (!current) return state;
          return {
            tabs: state.tabs.map((t) =>
              t.id === current.id ? { ...t, reloadToken: t.reloadToken + 1 } : t,
            ),
          };
        }),

      close: (id) =>
        set((state) => {
          const index = state.tabs.findIndex((t) => t.id === id);
          const victim = index === -1 ? undefined : state.tabs[index];
          if (index === -1 || !victim) return state;
          const tabs = state.tabs.filter((t) => t.id !== id);
          const recentlyClosed = rememberClosed(state.recentlyClosed, victim, index);
          if (tabs.length === 0) {
            // The window persists: closing the last tab opens the new-tab
            // page, exactly like a browser with one page left. The fresh
            // replacement is a birth, not a close — it enters no memory.
            const fresh = freshTab({ kind: "new" });
            return {
              tabs: [fresh],
              activeId: fresh.id,
              groups: {},
              recentlyClosed,
              discoveryQuery: null,
            };
          }
          if (state.activeId !== id) return { tabs, recentlyClosed, groups: tidyGroups(tabs, state.groups) };
          const next = tabs[index] ?? tabs[tabs.length - 1];
          return {
            tabs,
            activeId: next ? next.id : null,
            recentlyClosed,
            groups: tidyGroups(tabs, state.groups),
          };
        }),

      closeOthers: (id) =>
        set((state) => {
          const kept = state.tabs.find((t) => t.id === id);
          if (!kept) return state;
          // Recorded left-to-right so reopens walk the strip back in order.
          let recentlyClosed = state.recentlyClosed;
          state.tabs.forEach((t, index) => {
            if (t.id !== id) recentlyClosed = rememberClosed(recentlyClosed, t, index);
          });
          return {
            tabs: [kept],
            activeId: kept.id,
            recentlyClosed,
            groups: tidyGroups([kept], state.groups),
          };
        }),

      closeToTheRight: (id) =>
        set((state) => {
          const index = state.tabs.findIndex((t) => t.id === id);
          if (index === -1) return state;
          const doomed = state.tabs.slice(index + 1);
          if (doomed.length === 0) return state;
          let recentlyClosed = state.recentlyClosed;
          doomed.forEach((t, offset) => {
            recentlyClosed = rememberClosed(recentlyClosed, t, index + 1 + offset);
          });
          const tabs = state.tabs.slice(0, index + 1);
          const activeStays = tabs.some((t) => t.id === state.activeId);
          return {
            tabs,
            activeId: activeStays ? state.activeId : id,
            recentlyClosed,
            groups: tidyGroups(tabs, state.groups),
          };
        }),

      setActive: (id) => set({ activeId: id }),
      setDiscoveryQuery: (query) => set({ discoveryQuery: query }),

      newTabToTheRight: (id) =>
        set((state) => {
          const index = state.tabs.findIndex((t) => t.id === id);
          if (index === -1) return state;
          const resting = state.tabs.find((t) => tabKeyOf(t) === "new");
          if (resting) {
            // Singleton discipline (§6): the resting new tab moves over.
            const without = state.tabs.filter((t) => t.id !== resting.id);
            const at = Math.min(index, without.length);
            const tabs = normalizeOrder([...without.slice(0, at), resting, ...without.slice(at)]);
            return { tabs, activeId: resting.id };
          }
          const fresh = freshTab({ kind: "new" });
          const tabs = normalizeOrder([
            ...state.tabs.slice(0, index + 1),
            fresh,
            ...state.tabs.slice(index + 1),
          ]);
          return { tabs, activeId: fresh.id, discoveryQuery: null };
        }),

      duplicate: (id) =>
        set((state) => {
          const index = state.tabs.findIndex((t) => t.id === id);
          const origin = index === -1 ? undefined : state.tabs[index];
          if (index === -1 || !origin) return state;
          const clone: Tab = {
            id: newTabId(),
            history: [...origin.history],
            historyIndex: origin.historyIndex,
            reloadToken: 0,
            pinned: false,
          };
          // §52: the clone never lands inside the pinned block.
          const tabs = normalizeOrder([
            ...state.tabs.slice(0, index + 1),
            clone,
            ...state.tabs.slice(index + 1),
          ]);
          return { tabs, activeId: clone.id };
        }),

      togglePin: (id) =>
        set((state) => {
          const tab = state.tabs.find((t) => t.id === id);
          if (!tab) return state;
          const pinned = !tab.pinned;
          const moved = { ...tab, pinned, groupId: pinned ? undefined : tab.groupId };
          const tabs = normalize(state.tabs.map((t) => (t.id === id ? moved : t)));
          return { tabs, groups: tidyGroups(tabs, state.groups) };
        }),

      reopen: () =>
        set((state) => {
          const [mostRecent, ...rest] = state.recentlyClosed;
          if (!mostRecent) return state;
          const revived: Tab = {
            id: newTabId(),
            history: mostRecent.history,
            historyIndex: Math.min(mostRecent.historyIndex, mostRecent.history.length - 1),
            reloadToken: 0,
            pinned: mostRecent.pinned,
          };
          const at = Math.min(mostRecent.index, state.tabs.length);
          const tabs = normalizeOrder([...state.tabs.slice(0, at), revived, ...state.tabs.slice(at)]);
          return { tabs, activeId: revived.id, recentlyClosed: rest };
        }),

      addToNewGroup: (id) =>
        set((state) => {
          const tab = state.tabs.find((t) => t.id === id);
          if (!tab || tab.pinned) return state;
          const groupId = `g${Object.keys(state.groups).length + 1}-${Math.random()
            .toString(36)
            .slice(2, 6)}`;
          const color =
            GROUP_COLORS[Object.keys(state.groups).length % GROUP_COLORS.length] ?? "slate";
          const tabs = normalize(state.tabs.map((t) => (t.id === id ? { ...t, groupId } : t)));
          return {
            tabs,
            groups: { ...state.groups, [groupId]: { label: DEFAULT_GROUP_LABEL, color, collapsed: false } },
          };
        }),

      moveToGroup: (id, groupId) =>
        set((state) => {
          const tab = state.tabs.find((t) => t.id === id);
          if (!tab || tab.pinned || !state.groups[groupId]) return state;
          const tabs = normalize(state.tabs.map((t) => (t.id === id ? { ...t, groupId } : t)));
          return { tabs, groups: tidyGroups(tabs, state.groups) };
        }),

      removeFromGroup: (id) =>
        set((state) => {
          const tab = state.tabs.find((t) => t.id === id);
          if (!tab || !tab.groupId) return state;
          const tabs = state.tabs.map((t) => (t.id === id ? { ...t, groupId: undefined } : t));
          return { tabs, groups: tidyGroups(tabs, state.groups) };
        }),

      toggleGroupCollapse: (groupId) =>
        set((state) => {
          const group = state.groups[groupId];
          if (!group) return state;
          return {
            groups: { ...state.groups, [groupId]: { ...group, collapsed: !group.collapsed } },
          };
        }),

      renameGroup: (groupId, label) =>
        set((state) => {
          const group = state.groups[groupId];
          if (!group) return state;
          const clean = label.trim().slice(0, MAX_GROUP_LABEL);
          if (!clean) return state;
          return { groups: { ...state.groups, [groupId]: { ...group, label: clean } } };
        }),
    }),
    {
      name: "zamin-panel.tabs",
      version: 2,
      partialize: (state) => ({
        tabs: state.tabs,
        activeId: state.activeId,
        groups: state.groups,
      }),
      migrate: (persisted) => {
        // v1 stored destination-keyed tabs without ids. Every tab gets a
        // fresh id; the stored activeKey (a destination key) maps to the
        // first tab resting there.
        const raw = (persisted ?? {}) as Record<string, unknown>;
        const tabs = sanitizeTabs(raw.tabs);
        if (!tabs) return { tabs: [], activeId: null, groups: {} };
        const groups = sanitizeGroups(raw.groups);
        const withRealGroups = tabs.map((t) =>
          t.groupId && groups[t.groupId] ? t : { ...t, groupId: undefined },
        );
        const stored =
          typeof raw.activeKey === "string"
            ? tabs.find((t) => tabKeyOf(t) === raw.activeKey)?.id
            : undefined;
        const storedId = typeof raw.activeId === "string" ? raw.activeId : undefined;
        const active =
          tabs.find((t) => t.id === (storedId ?? stored))?.id ?? tabs[0]?.id ?? null;
        return { tabs: withRealGroups, activeId: active, groups };
      },
      merge: (persisted, current) => {
        const raw = (persisted ?? {}) as Record<string, unknown>;
        const tabs = sanitizeTabs(raw.tabs);
        if (!tabs) return current;
        const groups = sanitizeGroups(raw.groups);
        // Cross-store hygiene: memberships without groups, groups without
        // members — neither survives rehydration.
        const withRealGroups = tabs.map((t) =>
          t.groupId && groups[t.groupId] ? t : { ...t, groupId: undefined },
        );
        const surviving = tidyGroups(withRealGroups, groups);
        const stored = typeof raw.activeId === "string" ? raw.activeId : null;
        const active = withRealGroups.find((t) => t.id === stored) ?? withRealGroups[0];
        return {
          ...current,
          tabs: withRealGroups,
          groups: surviving,
          activeId: active ? active.id : null,
        };
      },
    },
  ),
);
