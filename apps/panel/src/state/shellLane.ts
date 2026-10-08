// The content side's single lane into the Rust shell (ADR-0033):
// content says "navigate me"; the host decides identity and renders.
import { invoke } from "@tauri-apps/api/core";
import type { Destination } from "./destinations";

export async function navigateHost(destination: Destination): Promise<void> {
  if (!("__TAURI_INTERNALS__" in window)) return; // dev bridge: local only
  try {
    await invoke("shell_tab_navigate", { destination });
  } catch {
    // Dev-bridge contexts have no shell model; navigation stays local.
  }
}

export async function tabActionHost(action: "back" | "forward" | "reload"): Promise<void> {
  if (!("__TAURI_INTERNALS__" in window)) return;
  try {
    await invoke("shell_tab_action", { action });
  } catch {
    // as above
  }
}
