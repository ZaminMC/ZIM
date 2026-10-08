// Tabs (ADR-0015, ADR-0016, ADR-0018): ordered browser tabs of typed
// destinations, per-tab destination history (§59), a reload token (§60),
// and singleton NAVIGATION identity (§61: opening a resting destination
// focuses it). Tab state is PANEL-LOCAL storage, never daemon state.
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
// ADR-0018 completes the machinery: DRAG REORDER (tabs are operators —
// the pinned head is clamped, dragging out of a group leaves it) and the
// §50 WINDOW MACHINERY. Moving a tab to a new window writes a handoff
// slot and the opener opens a second ZaminPanel context that claims the
// slot by hash at boot. The tab is not closed (no recently-closed entry)
// and the server behind it is never touched — the UI window is only a
// client; the process stays daemon-owned. Windows own their strips: the
// persistence is keyed per browsing context, so two windows coexist
// without clobbering each other's localStorage.
//
// Invariants: pinned tabs always sit at the head of the strip (§52),
// each block in stable order; a group's members sit together (§49);
// every mutation re-normalizes.

import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import { tabKey, type Destination, type TabKey } from "./destinations";
import { currentWindowId } from "./windowIdentity";

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

// --- per-window storage (ADR-0018, §50) ---------------------------------------

/** The pre-ADR-0018 shared strip key. Adopted once by the first fresh
 *  window, then retired so the next window is born fresh, not a thief. */
export const LEGACY_TABS_KEY = "zamin-panel.tabs";
/** Strips are private per browsing context. The `@` keeps them distinct
 *  from the legacy exact key while sharing its namespace. */
const STRIP_PREFIX = "zamin-panel.tabs@";
const REGISTRY_KEY = "zamin-panel.windows";
export const HANDOFF_PREFIX = "zamin-panel.tab-handoff:";
/** A handoff is claimed by a window opening NOW, not by one opened later. */
export const HANDOFF_TTL_MS = 60_000;
/** Stale windows' strips are pruned after two idle weeks. */
const REGISTRY_PRUNE_AFTER_MS = 14 * 24 * 60 * 60 * 1000;

export const stripKeyFor = (windowId: string): string => `${STRIP_PREFIX}${windowId}`;
export const stripKey = (): string => stripKeyFor(currentWindowId());

/** The handoff payload the origin writes and the child claims. The tab
 *  travels without its id (the child mints one) and without its group
 *  (groups are strip-local; membership cannot travel). */
export interface TabHandoff {
  version: 1;
  handoffId: string;
  issuedAt: number;
  history: Destination[];
  historyIndex: number;
  pinned: boolean;
}

function isHandoff(value: unknown): value is TabHandoff {
  if (!value || typeof value !== "object") return false;
  const v = value as Record<string, unknown>;
  return (
    v.version === 1 &&
    typeof v.handoffId === "string" &&
    v.handoffId !== "" &&
    typeof v.issuedAt === "number" &&
    Number.isFinite(v.issuedAt) &&
    Array.isArray(v.history) &&
    v.history.length > 0 &&
    typeof v.historyIndex === "number" &&
    Number.isInteger(v.historyIndex) &&
    typeof v.pinned === "boolean"
  );
}

const perWindowStorage = {
  getItem: (_name: string): string | null => {
    const own = localStorage.getItem(stripKey());
    if (own !== null) return own;
    // One-time adoption: the first fresh window inherits the shared
    // pre-window strip and retires it, so the second window is born
    // fresh instead of stealing the same tabs.
    const legacy = localStorage.getItem(LEGACY_TABS_KEY);
    if (legacy !== null) {
      localStorage.removeItem(LEGACY_TABS_KEY);
      return legacy;
    }
    return null;
  },
  setItem: (_name: string, value: string): void => {
    localStorage.setItem(stripKey(), value);
  },
  removeItem: (_name: string): void => {
    localStorage.removeItem(stripKey());
  },
};

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
  /** §55: open a bookmark in a NEW tab — an explicit request, so the
   *  resting new-tab page is not reused; the destination travels with a
   *  fresh view while the singleton rule stays a NAVIGATE-only rule. */
  openInNewTab: (destination: Destination) => void;
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
  /** §90's drag reorder: `insertion` is the visual slot in the current
   *  strip (0..tabs.length). §52 clamps: a pinned tab drags within the
   *  pinned block, a free tab never lands before it. Dragging out of a
   *  group's reach leaves the group; a drop never GAINS membership. */
  reorder: (id: TabId, insertion: number) => void;
  /** §50: hand the tab to a new ZaminPanel window. Writes the handoff
   *  slot and removes the tab here (a move is not a close — no
   *  recently-closed entry). Returns the handoff id for the opener, or
   *  null when refused (the last tab cannot leave; denied storage). */
  moveToNewWindow: (id: TabId) => string | null;
  /** The undo of a blocked move: the popup never opened, the tab comes
   *  home. Group membership survives only if the group still does. */
  restoreMoved: (tab: Tab, index: number, focus: boolean) => void;
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
    case "console":
      return typeof v.serverId === "string" && v.serverId !== ""
        ? { kind: "console", serverId: v.serverId }
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
    (set, get) => ({
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

      openInNewTab: (destination) =>
        set((state) => {
          const fresh = freshTab(destination);
          return {
            tabs: [...state.tabs, fresh],
            activeId: fresh.id,
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

      reorder: (id, insertion) =>
        set((state) => {
          const from = state.tabs.findIndex((t) => t.id === id);
          if (from === -1) return state;
          const slot = Math.max(0, Math.min(Math.floor(insertion), state.tabs.length));
          if (slot === from || slot === from + 1) return state; // dropped on itself
          const moving = state.tabs[from];
          if (!moving) return state;
          const without = state.tabs.filter((t) => t.id !== id);
          let at = slot > from ? slot - 1 : slot;
          // §52: the pinned block owns the head. A pinned tab drags
          // within the block (0..pinnedCount — the others' block plus its
          // tail); a free tab never lands before the block.
          const pinnedCount = without.filter((t) => t.pinned).length;
          at = moving.pinned
            ? Math.min(at, pinnedCount)
            : Math.max(at, pinnedCount);
          const left = at > 0 ? without[at - 1] : undefined;
          const right = without[at];
          let groupId = moving.groupId;
          if (groupId !== undefined) {
            const stays = left?.groupId === groupId || right?.groupId === groupId;
            if (!stays) groupId = undefined; // dragged out of the group's reach
          }
          const moved: Tab = { ...moving, groupId };
          const tabs = normalize([...without.slice(0, at), moved, ...without.slice(at)]);
          return {
            tabs,
            groups:
              moving.groupId !== undefined && groupId === undefined
                ? tidyGroups(tabs, state.groups)
                : state.groups,
          };
        }),

      moveToNewWindow: (id) => {
        const state = get();
        const index = state.tabs.findIndex((t) => t.id === id);
        const victim = index === -1 ? undefined : state.tabs[index];
        if (!victim || state.tabs.length === 1) return null; // the last tab cannot leave
        const handoffId = `h${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
        const handoff: TabHandoff = {
          version: 1,
          handoffId,
          issuedAt: Date.now(),
          history: victim.history,
          historyIndex: victim.historyIndex,
          pinned: victim.pinned === true,
        };
        try {
          localStorage.setItem(`${HANDOFF_PREFIX}${handoffId}`, JSON.stringify(handoff));
        } catch {
          return null; // denied storage cannot hand anything over
        }
        // The move is not a close (§50): the tab still exists, in the
        // window that is about to open — it enters no close memory.
        const tabs = state.tabs.filter((t) => t.id !== id);
        const focusMoves = state.activeId === id;
        const next = focusMoves ? (tabs[index] ?? tabs[tabs.length - 1]) : undefined;
        set({
          tabs,
          ...(focusMoves ? { activeId: next ? next.id : null } : {}),
          groups: tidyGroups(tabs, state.groups),
        });
        return handoffId;
      },

      restoreMoved: (tab, index, focus) => {
        set((state) => {
          if (state.tabs.some((t) => t.id === tab.id)) return state; // never duplicate
          const groupId =
            tab.groupId !== undefined && state.groups[tab.groupId] ? tab.groupId : undefined;
          const restored: Tab = { ...tab, groupId };
          const at = Math.max(0, Math.min(index, state.tabs.length));
          const tabs = normalize([...state.tabs.slice(0, at), restored, ...state.tabs.slice(at)]);
          return {
            tabs,
            ...(focus ? { activeId: restored.id } : {}),
            groups: tidyGroups(tabs, state.groups),
          };
        });
      },
    }),
    {
      name: LEGACY_TABS_KEY,
      version: 3,
      storage: createJSONStorage(() => perWindowStorage),
      partialize: (state) => ({
        tabs: state.tabs,
        activeId: state.activeId,
        groups: state.groups,
      }),
      migrate: (persisted) => {
        // v1 stored destination-keyed tabs without ids. Every tab gets a
        // fresh id; the stored activeKey (a destination key) maps to the
        // first tab resting there. v2/v3 carry ids and an activeId; the
        // shapes are identical from here on (v3 only moved the storage
        // key per window — the payload did not change).
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

// --- boot: the §50 handoff and the window registry -----------------------------

/** The child side of the §50 move. A context born with `#handoff=<id>` in
 *  its URL claims the slot that carries that exact id: the moved tab
 *  becomes this window's whole strip (active, pinned state preserved —
 *  group membership cannot travel). The slot is consumed in the same
 *  breath; a torn, foreign, or stale slot is refused, never half-claimed.
 *  Returns whether a tab was claimed. */
export function claimHandoff(hash: string | null | undefined): boolean {
  const match = typeof hash === "string" ? hash.match(/handoff=([A-Za-z0-9-]+)/) : null;
  const handoffId = match?.[1];
  if (!handoffId) return false;
  const key = `${HANDOFF_PREFIX}${handoffId}`;
  let slot: TabHandoff | null = null;
  try {
    const raw = localStorage.getItem(key);
    if (raw !== null) localStorage.removeItem(key); // a slot is claimed once
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      if (isHandoff(parsed)) slot = parsed;
    }
  } catch {
    return false; // a torn slot is refused, not half-claimed
  }
  if (!slot || slot.handoffId !== handoffId) return false;
  if (Date.now() - slot.issuedAt > HANDOFF_TTL_MS) return false; // stale hand, no tab
  const history: Destination[] = [];
  for (const entry of slot.history) {
    const dest = sanitizeDestination(entry);
    if (!dest) return false;
    history.push(dest);
  }
  if (history.length === 0) return false;
  const index = Math.max(0, Math.min(Math.floor(slot.historyIndex), history.length - 1));
  const tab: Tab = {
    id: newTabId(),
    history,
    historyIndex: index,
    reloadToken: 0,
    pinned: slot.pinned || undefined,
  };
  useTabs.setState({ tabs: [tab], activeId: tab.id, groups: {}, discoveryQuery: null });
  return true;
}

/** Sweep handoff slots nobody claimed — a blocked popup, a crashed child.
 *  Bounded housekeeping, run once per boot. */
function sweepStaleHandoffs(): void {
  const now = Date.now();
  const doomed: string[] = [];
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (key !== null && key.startsWith(HANDOFF_PREFIX)) doomed.push(key);
  }
  for (const key of doomed) {
    try {
      const raw = localStorage.getItem(key);
      const parsed: unknown = raw === null ? null : JSON.parse(raw);
      if (!isHandoff(parsed) || now - parsed.issuedAt > HANDOFF_TTL_MS) {
        localStorage.removeItem(key);
      }
    } catch {
      localStorage.removeItem(key); // torn slots have no claim on memory
    }
  }
}

/** Boot housekeeping for THIS window: touch the registry, prune windows
 *  idle past the horizon (their strips go with them), sweep stale
 *  handoffs, and force one persist write so a window that boots and
 *  reloads without touching anything still finds its own strip. */
export function bootWindow(): void {
  const id = currentWindowId();
  let registry: Record<string, unknown> = {};
  let usable = true;
  try {
    const raw = localStorage.getItem(REGISTRY_KEY);
    if (raw !== null) {
      const parsed: unknown = JSON.parse(raw);
      if (parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)) {
        registry = parsed as Record<string, unknown>;
      } else {
        usable = false; // a torn registry is rebuilt; strips are not nuked
      }
    }
  } catch {
    usable = false;
  }
  if (usable) {
    const now = Date.now();
    for (const [wid, last] of Object.entries(registry)) {
      if (typeof last === "number" && now - last > REGISTRY_PRUNE_AFTER_MS) {
        delete registry[wid];
        try {
          localStorage.removeItem(stripKeyFor(wid));
        } catch {
          // Denied storage: the registry entry still goes.
        }
      }
    }
  } else {
    registry = {};
  }
  registry[id] = Date.now();
  try {
    localStorage.setItem(REGISTRY_KEY, JSON.stringify(registry));
    sweepStaleHandoffs();
  } catch {
    // Denied storage: this window still works, its strip just stays private.
  }
  useTabs.setState({}); // one touch → the strip persists under this window's key
}
