// Bookmarks (§55, ADR-0023): the bar under the tool bar behaves like a
// browser bookmark bar. A bookmark holds a TYPED destination — never a
// string — because the founder's identity discipline (§61) applies here
// too: a bookmark to a server keeps being a bookmark to that server
// through renames, port changes, and window moves; opening it goes
// through the tabs store, so §61 singleton focus is inherited, not
// reimplemented.
//
// Scope: bookmarks are PANEL-LOCAL, persisted across windows (Chrome's
// model — the bar is shared, the tabs are not). The Dutchmen
// conversation kind has its room reserved in the same union: when the
// agent runtime lands, a conversation bookmark needs no migration — the
// creation surfaces will simply learn the new destination kind.

import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import type { Destination } from "./destinations";
import { tabKey } from "./destinations";

export type BookmarkId = string;

export interface Bookmark {
  id: BookmarkId;
  /** The operator's label — defaults from the destination, editable. */
  label: string;
  /** The typed destination; identity, not an address. */
  destination: Destination;
  addedAt: number;
}

interface BookmarksState {
  items: Bookmark[];
  /** §55: the bar is toggleable, like a browser's (Ctrl+Shift+B). */
  barVisible: boolean;
  add: (destination: Destination, label: string) => void;
  remove: (id: BookmarkId) => void;
  removeDestination: (destination: Destination) => void;
  rename: (id: BookmarkId, label: string) => void;
  move: (id: BookmarkId, toIndex: number) => void;
  toggleBar: () => void;
}

const MAX_LABEL = 48;

let idSeq = 0;
const newBookmarkId = (): BookmarkId =>
  `bm-${(++idSeq).toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

/** Torn-storage guard: a persisted row is a bookmark only when it names
 *  a destination the shell can still open. Unknown kinds are dropped at
 *  load, never rendered. */
function isBookmark(value: unknown): value is Bookmark {
  if (!value || typeof value !== "object") return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v.id === "string" &&
    v.id !== "" &&
    typeof v.label === "string" &&
    typeof v.addedAt === "number" &&
    Number.isFinite(v.addedAt) &&
    typeof v.destination === "object" &&
    v.destination !== null &&
    typeof (v.destination as Record<string, unknown>).kind === "string"
  );
}

/** The key two bookmarks would share: the destination's tab key. §55
 *  preserves destination identity, so one destination holds one
 *  bookmark — a second add is a no-op, and the star reflects membership
 *  instead of stacking rows. */
export const bookmarkKeyOf = (bookmark: Bookmark): string => tabKey(bookmark.destination);

export const useBookmarks = create<BookmarksState>()(
  persist(
    (set) => ({
      items: [],
      barVisible: true,

      add: (destination, label) =>
        set((state) => {
          const key = tabKey(destination);
          if (state.items.some((item) => bookmarkKeyOf(item) === key)) return state;
          const item: Bookmark = {
            id: newBookmarkId(),
            label: label.trim().slice(0, MAX_LABEL) || "Untitled",
            destination,
            addedAt: Date.now(),
          };
          return { items: [...state.items, item] };
        }),

      remove: (id) =>
        set((state) => ({ items: state.items.filter((item) => item.id !== id) })),

      removeDestination: (destination) =>
        set((state) => {
          const key = tabKey(destination);
          return { items: state.items.filter((item) => bookmarkKeyOf(item) !== key) };
        }),

      rename: (id, label) =>
        set((state) => ({
          items: state.items.map((item) =>
            item.id === id
              ? { ...item, label: label.trim().slice(0, MAX_LABEL) || item.label }
              : item,
          ),
        })),

      move: (id, toIndex) =>
        set((state) => {
          const from = state.items.findIndex((item) => item.id === id);
          if (from === -1) return state;
          const items = [...state.items];
          const [moved] = items.splice(from, 1);
          if (!moved) return state;
          const clamped = Math.max(0, Math.min(toIndex, items.length));
          items.splice(clamped, 0, moved);
          return { items };
        }),

      toggleBar: () => set((state) => ({ barVisible: !state.barVisible })),
    }),
    {
      name: "zamin-panel.bookmarks",
      storage: createJSONStorage(() => localStorage),
      version: 1,
      migrate: (persisted) => {
        if (!persisted || typeof persisted !== "object") return { items: [], barVisible: true };
        const v = persisted as { items?: unknown; barVisible?: unknown };
        return {
          items: Array.isArray(v.items) ? v.items.filter(isBookmark) : [],
          barVisible: typeof v.barVisible === "boolean" ? v.barVisible : true,
        };
      },
      merge: (persisted, current) => {
        const v = persisted as { items?: unknown; barVisible?: unknown };
        return {
          ...current,
          items: Array.isArray(v.items) ? v.items.filter(isBookmark) : current.items,
          barVisible:
            typeof v.barVisible === "boolean" ? v.barVisible : current.barVisible,
        };
      },
    },
  ),
);

/** Is this destination bookmarked? (The address bar star's state.) */
export function isBookmarked(items: Bookmark[], destination: Destination): boolean {
  const key = tabKey(destination);
  return items.some((item) => bookmarkKeyOf(item) === key);
}
