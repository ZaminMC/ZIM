// The Tauri integration seam: everything the webview asks the desktop host
// to do, gathered in one browser-safe module. Development runs in a plain
// browser against the dev bridge, so every helper degrades to null there —
// call sites decide what "unavailable" looks like (STYLE-GUIDE: honest
// states, never a fake success).

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

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
