// The frame's IPC bridge — typed front for the shell host's commands
// and events (shell/host.rs). Everything here is the VIEW side of
// ADR-0033: the model is authoritative, this only speaks for it.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Running under the desktop host (false = browser dev bridge). */
export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// -- Snapshot types (mirror shell::host) -------------------------------------

export interface Slot {
  id: number;
  x: number;
  width: number;
  pinned: boolean;
  closing: boolean;
}

export interface TabView {
  id: number;
  title: string;
  url: string;
  pinned: boolean;
  muted: boolean;
  group: number | null;
  active: boolean;
  can_back: boolean;
  can_forward: boolean;
}

export interface GroupView {
  id: number;
  label: string;
  color: number;
  collapsed: boolean;
}

export interface BookmarkView {
  id: string;
  title: string;
  destination: { kind: string; [k: string]: unknown };
}

export interface Snapshot {
  window: string;
  strip_width: number;
  header_height: number;
  bookmarks_bar_visible: boolean;
  slots: Slot[];
  tabs: TabView[];
  groups: GroupView[];
  active: number | null;
  address: string;
  can_reopen_closed: boolean;
  bookmarks: BookmarkView[];
}

// -- Commands -----------------------------------------------------------------

/** A browser command (the ported command ID space, shell/commands.rs). */
export const shellCommand = (id: number, arg?: Record<string, unknown>): Promise<void> =>
  isTauri()
    ? invoke("shell_command", { id, arg: arg ?? null })
    : demoActive()
      ? (demoCommand(id, arg ?? null), Promise.resolve())
      : Promise.resolve();

export type CommitOutcome =
  | { kind: "navigated" }
  | { kind: "join"; host: string | null; port: number }
  | { kind: "query"; text: string };

export type AddressRequest =
  | { Internal: { kind: string; [k: string]: unknown } }
  | { Join: { host: string | null; port: number } }
  | { Query: string };

export const omniboxCommit = (text: string): Promise<CommitOutcome> =>
  isTauri()
    ? invoke<CommitOutcome>("shell_omnibox_commit", { text })
    : Promise.resolve({ kind: "query", text });

export const omniboxClassify = (text: string): Promise<AddressRequest | null> =>
  isTauri() ? invoke("shell_omnibox_classify", { text }) : Promise.resolve(null);

/** The tab context menu. Under the desktop host this shows the NATIVE
 *  popup (the frame band clips any DOM menu) and resolves true; the
 *  browser demo has no OS menu, resolves false so the view can draw a
 *  DOM stand-in. */
export const tabContextMenu = (tabId: number): Promise<boolean> =>
  isTauri()
    ? invoke("shell_tab_menu", { tabId }).then(() => true)
    : Promise.resolve(false);

export const bootFrame = (): Promise<Snapshot | null> =>
  isTauri()
    ? invoke<Snapshot>("shell_boot")
    : demoActive()
      ? Promise.resolve(demoSnapshot())
      : new Promise((resolve) => {
          // A browser page with neither the host nor ?demo: resolve null
          // after a beat so the frame can show its honest boot error
          // instead of an eternal ellipsis.
          window.setTimeout(() => resolve(null), 1200);
        });

export const reportFrameSize = (width: number, height: number): Promise<void> =>
  isTauri()
    ? invoke("shell_window_resized", { width, height })
    : demoActive()
      ? ((demo.strip_width = width), demoEmit(), Promise.resolve())
      : Promise.resolve();

/** The drag session — phase events into the ported TabDragController. */
export const shellDrag = (
  phase: "start" | "move" | "drop" | "cancel",
  payload: { tab_id?: number; x?: number; y?: number; screen_x?: number; screen_y?: number } = {},
): Promise<void> =>
  isTauri()
    ? invoke("shell_drag", {
        phase,
        tab_id: payload.tab_id ?? null,
        x: payload.x ?? null,
        y: payload.y ?? null,
        screen_x: payload.screen_x ?? null,
        screen_y: payload.screen_y ?? null,
      })
    : Promise.resolve();

// -- Events -------------------------------------------------------------------

export function onSnapshot(handler: (snap: Snapshot) => void): Promise<UnlistenFn> {
  if (demoActive()) return demoOnSnapshot(handler);
  return listen<Snapshot>("shell://snapshot", (event) => handler(event.payload));
}

export function onFocusAddress(handler: () => void): Promise<UnlistenFn> {
  return listen("shell://focus-address", () => handler());
}

/** The native menu's "Add to new group…": the label is the operator's
 *  to type, so the host bounces here and the frame asks, then lands the
 *  group command itself. */
export function onAskGroupLabel(
  handler: (tabId: number) => void,
): Promise<UnlistenFn> {
  return listen<{ tab_id: number }>("shell://ask-group-label", (event) =>
    handler(event.payload.tab_id),
  );
}

// -- Demo mode (?demo, browser only) -----------------------------------------
// The frame is a pure view of a snapshot, so a browser page can wear the
// real UI: `frame.html?demo` boots against an in-page fixture that obeys
// the same command IDs. UI iteration and screenshots never need the
// desktop host. Production and tests never land here (Tauri present /
// no ?demo flag).

function demoActive(): boolean {
  return (
    !isTauri() &&
    typeof window !== "undefined" &&
    new URLSearchParams(window.location.search).has("demo")
  );
}

interface DemoTab {
  id: number;
  title: string;
  address: string;
  pinned: boolean;
  muted: boolean;
  group: number | null;
  can_back: boolean;
  can_forward: boolean;
}

const demo = {
  strip_width: 1920,
  next_id: 5,
  active: 2,
  bar_visible: false,
  tabs: [
    { id: 1, title: "Set up the docs", address: "zaminpanel://settings/", pinned: true, muted: false, group: null, can_back: true, can_forward: false },
    { id: 2, title: "New tab", address: "zaminpanel://new", pinned: false, muted: false, group: null, can_back: false, can_forward: false },
    { id: 3, title: "Server survival", address: "zaminpanel://server/survival", pinned: false, muted: false, group: 1, can_back: true, can_forward: false },
    { id: 4, title: "Console hub", address: "zaminpanel://console/hub", pinned: false, muted: false, group: 1, can_back: false, can_forward: true },
  ] as DemoTab[],
  groups: [{ id: 1, label: "survival", color: 1, collapsed: false }],
  bookmarks: [
    { id: "b1", title: "Fleet", destination: { kind: "servers" } },
    { id: "b2", title: "survival console", destination: { kind: "console", serverId: "survival" } },
    { id: "b3", title: "Jobs", destination: { kind: "jobs" } },
  ],
  closed: [] as DemoTab[],
};

function demoSnapshot(): Snapshot {
  // The layout law's demo echo — Chromium widths, the strip's leading
  // inset (kTabStripPadding), the caption area reserved, the shrink law
  // (inactive to minimum, then the active, then clamped). A faithful
  // mirror of shell/layout.rs at fixture scale.
  const overlap = 18;
  const LEADING = 6; // STRIP_PADDING
  const NEW_TAB_W = 36; // NEW_TAB_BUTTON_W
  const CONTROLS_W = 138; // WINDOW_CONTROLS_W (3 × 46)
  const STANDARD = 256; // standard_width()
  const PINNED_W = 40; // pinned_width()
  const MIN_INACTIVE = 32; // min_inactive_width()
  const MIN_ACTIVE = 32; // min_active_width()

  const ordered = [...demo.tabs].sort((a, b) => Number(b.pinned) - Number(a.pinned));
  const budget = Math.max(demo.strip_width - LEADING - NEW_TAB_W - CONTROLS_W, 0);
  const widths: number[] = ordered.map((t) => (t.pinned ? PINNED_W : STANDARD));
  const preferred =
    widths.reduce((a, b) => a + b, 0) - overlap * Math.max(ordered.length - 1, 0);
  const widthAt = (i: number): number => widths[i] ?? STANDARD;
  const isPinned = (i: number): boolean => ordered[i]?.pinned ?? false;

  if (preferred > budget && ordered.length > 0) {
    const activeIndex = ordered.findIndex((t) => t.id === demo.active);
    // Inactive tabs down to their minimum first…
    for (let i = 0; i < ordered.length; i++) {
      if (!isPinned(i) && i !== activeIndex) widths[i] = MIN_INACTIVE;
    }
    let total =
      widths.reduce((a, b) => a + b, 0) - overlap * Math.max(ordered.length - 1, 0);
    // …then the active tab, if even that is not enough.
    if (total > budget && activeIndex >= 0 && !isPinned(activeIndex)) {
      widths[activeIndex] = MIN_ACTIVE;
      total =
        widths.reduce((a, b) => a + b, 0) - overlap * Math.max(ordered.length - 1, 0);
    }
    // Leftover: the active tab first, then evenly, up to standard width.
    let free = budget - total;
    for (let i = 0; i < ordered.length && free > 0; i++) {
      if (isPinned(i)) continue;
      const room = STANDARD - widthAt(i);
      const give = i === activeIndex ? Math.min(room, free) : Math.min(room, free * 0.5);
      widths[i] = widthAt(i) + give;
      free -= give;
    }
  }

  const slots: Snapshot["slots"] = [];
  let x = LEADING;
  ordered.forEach((tab, i) => {
    slots.push({ id: tab.id, x, width: widthAt(i), pinned: tab.pinned, closing: false });
    x += widthAt(i) - overlap;
  });
  const active = demo.tabs.find((t) => t.id === demo.active) ?? null;
  return {
    window: "main",
    strip_width: demo.strip_width,
    header_height: demo.bar_visible ? 111 : 83,
    bookmarks_bar_visible: demo.bar_visible,
    slots,
    tabs: demo.tabs.map((t) => ({
      id: t.id,
      title: t.title,
      url: t.address,
      pinned: t.pinned,
      muted: t.muted,
      group: t.group,
      active: t.id === demo.active,
      can_back: t.can_back,
      can_forward: t.can_forward,
    })),
    groups: demo.groups,
    active: demo.active,
    // The NTP law: a new tab carries no address at all.
    address: active ? (active.address === "zaminpanel://new" ? "" : active.address) : "",
    can_reopen_closed: demo.closed.length > 0,
    bookmarks: demo.bookmarks,
  };
}

const demoListeners = new Set<(snap: Snapshot) => void>();

function demoEmit(): void {
  const snap = demoSnapshot();
  window.setTimeout(() => demoListeners.forEach((fn) => fn(snap)), 0);
}

function demoOnSnapshot(handler: (snap: Snapshot) => void): Promise<UnlistenFn> {
  demoListeners.add(handler);
  window.setTimeout(() => handler(demoSnapshot()), 0);
  return Promise.resolve(() => demoListeners.delete(handler));
}

function demoCommand(id: number, arg: Record<string, unknown> | null): void {
  const byId = (tid: number) => demo.tabs.find((t) => t.id === tid);
  const indexOfActive = () => demo.tabs.findIndex((t) => t.id === demo.active);
  switch (id) {
    case 34014: { // NEW_TAB
      const tab: DemoTab = { id: demo.next_id++, title: "New tab", address: "zaminpanel://new", pinned: false, muted: false, group: null, can_back: false, can_forward: false };
      demo.tabs.splice(indexOfActive() + 1, 0, tab);
      demo.active = tab.id;
      break;
    }
    case 34015: { // CLOSE_TAB
      const tid = Number(arg?.tab_id ?? demo.active);
      const index = demo.tabs.findIndex((t) => t.id === tid);
      const [gone] = demo.tabs.splice(index, 1);
      if (gone) {
        demo.closed.push(gone);
        if (demo.active === tid) demo.active = demo.tabs[Math.min(index, demo.tabs.length - 1)]?.id ?? 0;
      }
      break;
    }
    case 34016: { // SELECT_NEXT
      const next = demo.tabs[(indexOfActive() + 1) % Math.max(demo.tabs.length, 1)];
      if (next) demo.active = next.id;
      break;
    }
    case 34017: { // SELECT_PREVIOUS
      const prev = demo.tabs[(indexOfActive() - 1 + demo.tabs.length) % Math.max(demo.tabs.length, 1)];
      if (prev) demo.active = prev.id;
      break;
    }
    case 34018: case 34019: case 34020: case 34021: // SELECT_TAB_0..7
    case 34022: case 34023: case 34024: case 34025: {
      const target = demo.tabs[id - 34018];
      if (target) demo.active = target.id;
      break;
    }
    case 34027: { // DUPLICATE_TAB
      const source = byId(Number(arg?.tab_id ?? demo.active));
      if (source) {
        const copy = { ...source, id: demo.next_id++ };
        demo.tabs.splice(indexOfActive() + 1, 0, copy);
        demo.active = copy.id;
      }
      break;
    }
    case 34028: { // RESTORE_TAB
      const back = demo.closed.pop();
      if (back) {
        demo.tabs.splice(Math.max(indexOfActive(), 0) + 1, 0, back);
        demo.active = back.id;
      }
      break;
    }
    case 50001: { // TOGGLE_PINNED
      const tab = byId(Number(arg?.tab_id ?? demo.active));
      if (tab) tab.pinned = !tab.pinned;
      break;
    }
    case 50003: { // TOGGLE_MUTE
      const tab = byId(Number(arg?.tab_id ?? demo.active));
      if (tab) tab.muted = !tab.muted;
      break;
    }
    case 40009: demo.bar_visible = !demo.bar_visible; break; // SHOW_BOOKMARK_BAR
    default: break; // window verbs, navigation, omnibox — nothing to mirror
  }
  demoEmit();
}
