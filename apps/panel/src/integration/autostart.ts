// Login autostart, webview side: the host does the platform work (XDG
// autostart entry / HKCU Run key), this module is the seam that stays
// honest outside Tauri — in a plain browser the toggle reports
// unavailable instead of pretending.

import { invokeIfTauri } from "./tauri";

export type AutostartStatus = { available: true; enabled: boolean } | { available: false };

export async function autostartStatus(): Promise<AutostartStatus> {
  // Host answers Some(bool); null (unknown or not Tauri) → unavailable.
  const enabled = await invokeIfTauri<boolean | null>("autostart_get");
  if (enabled === null) return { available: false };
  return { available: true, enabled };
}

/** Returns false when the toggle is unavailable or the host refused. */
export async function setAutostart(enabled: boolean): Promise<boolean> {
  const done: unknown = await invokeIfTauri("autostart_set", { enabled });
  // In Tauri the host resolves (unit) on success; null = outside Tauri or
  // the host refused, and the toggle must stay honest.
  return done !== null;
}
