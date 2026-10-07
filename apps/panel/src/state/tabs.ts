// Tabs (ADR-0015): ordered browser tabs of typed destinations, per-tab
// destination history (§59: only destination changes are history), a
// reload token (§60: rebuild the view, never the server process), and
// singleton identity (§6: one tab per server, one fleet page, one
// new-tab page). Tab state is PANEL-LOCAL storage, never daemon state.
//
// Navigation rule: navigating to a destination that already rests in some
// tab focuses that tab (identity discipline, §61) — navigation never
// duplicates identity. Otherwise the ACTIVE tab travels: its history
// grows, its derived key follows its current destination. A tab's key is
// therefore derived, not stored — exactly what destinations.ts promises.

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { tabKey, type Destination, type TabKey } from "./destinations";

export interface Tab {
  /** Destination history, oldest first; current = history[historyIndex]. */
  history: Destination[];
  historyIndex: number;
  /** §60 reload: bumped on demand; the mounted view keys off it and
   *  rebuilds. Never touches the server process. */
  reloadToken: number;
}

/** The tab's current destination, guarded against torn storage. */
export function tabDestination(tab: Tab): Destination {
  return tab.history[tab.historyIndex] ?? { kind: "new" };
}

export const tabKeyOf = (tab: Tab): TabKey => tabKey(tabDestination(tab));
export const canBack = (tab: Tab): boolean => tab.historyIndex > 0;
export const canForward = (tab: Tab): boolean => tab.historyIndex < tab.history.length - 1;

const freshTab = (destination: Destination): Tab => ({
  history: [destination],
  historyIndex: 0,
  reloadToken: 0,
});

const DEFAULT_TABS: Tab[] = [freshTab({ kind: "new" })];

interface TabsState {
  tabs: Tab[];
  activeKey: TabKey | null;
  /** The query dialect's landing spot: free text typed in the address bar
   *  opens the discovery page carrying this text. Ephemeral by design —
   *  a search refinement is not a destination change (§59). */
  discoveryQuery: string | null;
  navigate: (destination: Destination) => void;
  newTab: (query?: string) => void;
  back: () => void;
  forward: () => void;
  reload: () => void;
  close: (key: TabKey) => void;
  closeOthers: (key: TabKey) => void;
  closeToTheRight: (key: TabKey) => void;
  setActive: (key: TabKey) => void;
  setDiscoveryQuery: (query: string | null) => void;
}

/** The active tab, or the first tab when the stored key went stale. */
function activeOf(tabs: Tab[], activeKey: TabKey | null): Tab | undefined {
  return tabs.find((t) => tabKeyOf(t) === activeKey) ?? tabs[0];
}

/** Travel within one tab: truncate the forward tail, append the destination. */
function travel(tab: Tab, destination: Destination): Tab {
  const history = [...tab.history.slice(0, tab.historyIndex + 1), destination];
  return { ...tab, history, historyIndex: history.length - 1 };
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
    tabs.push({ history, historyIndex: index, reloadToken: token });
  }
  return tabs;
}

export const useTabs = create<TabsState>()(
  persist(
    (set) => ({
      tabs: DEFAULT_TABS.map((t) => ({ ...t })),
      activeKey: "new",
      discoveryQuery: null,

      navigate: (destination) =>
        set((state) => {
          const key = tabKey(destination);
          // Identity discipline: already resting somewhere? Focus it.
          const resting = state.tabs.find((t) => tabKeyOf(t) === key);
          if (resting) return { activeKey: key, discoveryQuery: null };
          const current = activeOf(state.tabs, state.activeKey);
          if (!current) {
            return { tabs: [freshTab(destination)], activeKey: key, discoveryQuery: null };
          }
          const from = tabKeyOf(current);
          const tabs = state.tabs.map((t) => (tabKeyOf(t) === from ? travel(t, destination) : t));
          return { tabs, activeKey: key, discoveryQuery: null };
        }),

      newTab: (query) =>
        set((state) => {
          const resting = state.tabs.some((t) => tabKeyOf(t) === "new");
          const tabs = resting ? state.tabs : [...state.tabs, freshTab({ kind: "new" })];
          return { tabs, activeKey: "new", discoveryQuery: query ?? null };
        }),

      back: () =>
        set((state) => {
          const current = activeOf(state.tabs, state.activeKey);
          if (!current || !canBack(current)) return state;
          const from = tabKeyOf(current);
          let nextKey: TabKey = from;
          const tabs = state.tabs.map((t) => {
            if (tabKeyOf(t) !== from) return t;
            const moved = { ...t, historyIndex: t.historyIndex - 1 };
            nextKey = tabKeyOf(moved);
            return moved;
          });
          return { tabs, activeKey: nextKey };
        }),

      forward: () =>
        set((state) => {
          const current = activeOf(state.tabs, state.activeKey);
          if (!current || !canForward(current)) return state;
          const from = tabKeyOf(current);
          let nextKey: TabKey = from;
          const tabs = state.tabs.map((t) => {
            if (tabKeyOf(t) !== from) return t;
            const moved = { ...t, historyIndex: t.historyIndex + 1 };
            nextKey = tabKeyOf(moved);
            return moved;
          });
          return { tabs, activeKey: nextKey };
        }),

      reload: () =>
        set((state) => {
          const current = activeOf(state.tabs, state.activeKey);
          if (!current) return state;
          const from = tabKeyOf(current);
          return {
            tabs: state.tabs.map((t) =>
              tabKeyOf(t) === from ? { ...t, reloadToken: t.reloadToken + 1 } : t,
            ),
          };
        }),

      close: (key) =>
        set((state) => {
          const index = state.tabs.findIndex((t) => tabKeyOf(t) === key);
          if (index === -1) return state;
          const wasActive = state.activeKey === key;
          const tabs = state.tabs.filter((_, i) => i !== index);
          if (tabs.length === 0) {
            // The window persists: closing the last tab opens the new-tab
            // page, exactly like a browser with one page left.
            return { tabs: DEFAULT_TABS.map((t) => ({ ...t })), activeKey: "new", discoveryQuery: null };
          }
          if (!wasActive) return { tabs };
          const next = tabs[index] ?? tabs[tabs.length - 1];
          return { tabs, activeKey: next ? tabKeyOf(next) : null };
        }),

      closeOthers: (key) =>
        set((state) => {
          const kept = state.tabs.find((t) => tabKeyOf(t) === key);
          if (!kept) return state;
          return { tabs: [kept], activeKey: key };
        }),

      closeToTheRight: (key) =>
        set((state) => {
          const index = state.tabs.findIndex((t) => tabKeyOf(t) === key);
          if (index === -1) return state;
          const tabs = state.tabs.slice(0, index + 1);
          const activeStays = tabs.some((t) => tabKeyOf(t) === state.activeKey);
          return { tabs, activeKey: activeStays ? state.activeKey : key };
        }),

      setActive: (key) => set({ activeKey: key }),
      setDiscoveryQuery: (query) => set({ discoveryQuery: query }),
    }),
    {
      name: "zamin-panel.tabs",
      version: 1,
      partialize: (state) => ({ tabs: state.tabs, activeKey: state.activeKey }),
      merge: (persisted, current) => {
        const raw = (persisted ?? {}) as Record<string, unknown>;
        const tabs = sanitizeTabs(raw.tabs);
        if (!tabs) return current;
        const stored = typeof raw.activeKey === "string" ? raw.activeKey : null;
        const active = tabs.find((t) => tabKeyOf(t) === stored) ?? tabs[0];
        return { ...current, tabs, activeKey: active ? tabKeyOf(active) : "new" };
      },
    },
  ),
);
