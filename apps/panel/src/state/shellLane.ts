// The content side's single lane into the Rust shell (ADR-0033):
// content says "navigate me"; the host decides identity and renders.
import { invokeHost } from "../integration/tauri";
import type { Destination } from "./destinations";

const NAVIGATE_ACTIVE = 50004;

export async function navigateHost(destination: Destination): Promise<void> {
  if (!("__TAURI_INTERNALS__" in window)) return; // dev bridge: local only
  try {
    await invokeHost("shell_tab_navigate", { destination });
  } catch (firstError) {
    // The normal path is tab-identity addressed (`shell_tab_navigate`).
    // Keep a host-side active-tab fallback for content webviews that race
    // their first hello or briefly carry a label the shell cannot parse.
    // This is still authoritative Rust navigation — never a local React
    // navigation — and makes a server card/open action recover instead of
    // silently doing nothing.
    try {
      await invokeHost("shell_command", {
        id: NAVIGATE_ACTIVE,
        arg: { destination },
      });
    } catch (fallbackError) {
      if (import.meta.env.DEV) {
        console.warn("ZIM navigation failed", { firstError, fallbackError });
      }
    }
  }
}

export async function tabActionHost(action: "back" | "forward" | "reload"): Promise<void> {
  if (!("__TAURI_INTERNALS__" in window)) return;
  try {
    await invokeHost("shell_tab_action", { action });
  } catch {
    // as above
  }
}
