// The content side's single lane into the Rust shell (ADR-0033):
// content says "navigate me"; the host decides identity and renders.
import { invokeHost } from "../integration/tauri";
import type { Destination } from "./destinations";

const NAVIGATE_ACTIVE = 50004;

/** True when the host accepted the navigation; false when every lane
 *  refused — the caller owes the operator words, never silence (a card
 *  that does "absolutely nothing" on click is the bug this return exists
 *  to kill). The dev bridge navigates locally and reports true. */
export async function navigateHost(destination: Destination): Promise<boolean> {
  if (!("__TAURI_INTERNALS__" in window)) return true; // dev bridge: local only
  try {
    await invokeHost("shell_tab_navigate", { destination });
    return true;
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
      return true;
    } catch (fallbackError) {
      if (import.meta.env.DEV) {
        console.warn("ZIM navigation failed", { firstError, fallbackError });
      }
      return false;
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

/** Open a destination in a NEW foreground tab (the console's own-tab
 *  affordance and the page verbs that must not replace the tab under
 *  the operator's hand). Returns true when the host opened it; the dev
 *  bridge has no shell model, so it reports false and the caller falls
 *  back to its local lane. */
export async function openHostTab(destination: Destination): Promise<boolean> {
  if (!("__TAURI_INTERNALS__" in window)) return false; // dev bridge: no host
  try {
    await invokeHost("shell_tab_open", { destination });
    return true;
  } catch (error) {
    if (import.meta.env.DEV) {
      console.warn("ZIM open-in-new-tab failed", { error });
    }
    return false;
  }
}
