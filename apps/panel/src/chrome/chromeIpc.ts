// The chrome layer's IPC bridge — typed front for the shell host's
// commands and events (shell/host.rs). Everything here is the VIEW side
// of ADR-0033: the model is authoritative, this only speaks for it.

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

/** A browser command (Chromium's ID space, shell/commands.rs). */
export const shellCommand = (id: number, arg?: Record<string, unknown>): Promise<void> =>
  isTauri() ? invoke("shell_command", { id, arg: arg ?? null }) : Promise.resolve();

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

export const bootChrome = (): Promise<Snapshot | null> =>
  isTauri() ? invoke<Snapshot>("shell_boot") : Promise.resolve(null);

export const reportChromeSize = (width: number, height: number): Promise<void> =>
  isTauri() ? invoke("shell_window_resized", { width, height }) : Promise.resolve();

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
  return listen<Snapshot>("shell://snapshot", (event) => handler(event.payload));
}

export function onFocusAddress(handler: () => void): Promise<UnlistenFn> {
  return listen("shell://focus-address", () => handler());
}
