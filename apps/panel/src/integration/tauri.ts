// The Tauri integration seam: everything the webview asks the desktop host
// to do, gathered in one browser-safe module. Development runs in a plain
// browser against the dev bridge, so every helper degrades to null there —
// call sites decide what "unavailable" looks like (STYLE-GUIDE: honest
// states, never a fake success).
//
// This file is the one importer the lint boundary sanctions; the values
// arrive through dynamic imports so a browser boot never loads them, and
// the types through import type (erased at build).

import type { invoke } from "@tauri-apps/api/core";
import type { listen } from "@tauri-apps/api/event";
import type { getCurrentWebview } from "@tauri-apps/api/webview";

export const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Invoke a host command outside Tauri → `null` instead of throwing. */
export async function invokeIfTauri<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T | null> {
  if (!isTauri) return null;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<T>(command, args);
  } catch {
    // A failing integration command must never take the caller down; the
    // host surfaces its own remediation through the returned error strings.
    return null;
  }
}

// -- The shell lane's seam ----------------------------------------------------
//
// The frame/content/popup documents speak to the host through HERE: the
// module imports @tauri-apps/api lazily and once, so no view file ever
// names the integration package (the lint boundary enforces it) and a
// browser-only boot never pays for it. Error semantics are the API's own
// — throw-through — because the shell lane's callers gate on isTauri
// themselves and own their failure stories.

/** A listener's off switch (tauri's UnlistenFn, named for the panel). */
export type Unlisten = () => void;

type TauriApi = {
  invoke: typeof invoke;
  listen: typeof listen;
  getCurrentWebview: typeof getCurrentWebview;
};

let apiPromise: Promise<TauriApi> | null = null;

function api(): Promise<TauriApi> {
  apiPromise ??= (async () => {
    const [core, event, webview] = await Promise.all([
      import("@tauri-apps/api/core"),
      import("@tauri-apps/api/event"),
      import("@tauri-apps/api/webview"),
    ]);
    return {
      invoke: core.invoke,
      listen: event.listen,
      getCurrentWebview: webview.getCurrentWebview,
    };
  })();
  return apiPromise;
}

/** Invoke a host command. Throws like `invoke` — the caller gates. */
export async function invokeHost<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke } = await api();
  return invoke<T>(command, args);
}

/** Subscribe to a host event; resolves to the listener's off switch.
 *  The seam unwraps tauri's Event envelope: handlers take the payload
 *  itself. The generic is input-only by nature — an event's payload
 *  type exists only in the handler's parameter — which is exactly the
 *  shape this seam exists to state once instead of at every call site.
 *
 *  THE SCOPING LAW (the bug this fixed): the subscription rides the
 *  CURRENT WEBVIEW's target (`getCurrentWebview().listen`), not the
 *  global listener pool. Tauri's `emit_to(label)` delivers to the
 *  addressed webview's listeners — and ALSO to every `EventTarget::Any`
 *  listener (tauri 2.12 `match_any_or_filter`: `*target == Any ||
 *  filter(target)`), so the global `listen()` heard EVERY webview's
 *  events: each tab's `shell://tab` destination payload landed in all
 *  of them and the last write won — one tab's Settings rendered inside
 *  a sibling's New tab, a Console navigation never reached the screen.
 *  Chromium's law is the opposite — a WebContents receives only its own
 *  messages (Mojo pipes are per-WebContents, never broadcast) — and
 *  this is that law: the frame hears `shell://snapshot`, a content
 *  webview hears only its own `shell://tab`, the overlay only its own
 *  popup events. */
// eslint-disable-next-line @typescript-eslint/no-unnecessary-type-parameters
export async function listenHost<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<Unlisten> {
  const { getCurrentWebview } = await api();
  return getCurrentWebview().listen<T>(event, (e) => handler(e.payload));
}
