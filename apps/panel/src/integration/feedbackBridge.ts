// The feedback lane's desktop seam (ADR-0028), beside the updater's: the
// webview composes the report and decides the route, and this module is
// the only place that talks to the desktop plugins performing the OS
// calls — opening the browser at the prepared GitHub URL, and handing a
// pasted screenshot back to the clipboard for the GitHub form's own
// paste-attach. Development runs in a plain browser, so the real bridge
// resolves to null there; call sites decide what "unavailable" looks like
// (STYLE-GUIDE: honest states, never a fake success).
//
// Every method answers a Result instead of throwing (§81): a feedback
// send that fails must be a visible state, never a silent one.

import { isTauri } from "./tauri";

export type BridgeResult<T> = { ok: true; value: T } | { ok: false; message: string };

function failure(what: string, error: unknown): BridgeResult<never> {
  const message = error instanceof Error ? error.message : String(error);
  return { ok: false, message: `${what}: ${message}` } as never;
}

/** The OS calls the feedback page may ask for. Injected into the page, so
 *  its routing decisions are testable without a desktop host. */
export interface FeedbackBackend {
  /** Open a URL in the operator's default browser. */
  openUrl(url: string): Promise<BridgeResult<null>>;
  /** Put a pasted screenshot (PNG bytes) back on the clipboard, so the
   *  GitHub issue form's own paste-attach can take it. */
  copyImage(png: Uint8Array): Promise<BridgeResult<null>>;
}

export function realFeedbackBackend(): Promise<FeedbackBackend | null> {
  if (!isTauri) return Promise.resolve(null);
  return Promise.resolve({
    async openUrl(url) {
      try {
        const opener = await import("@tauri-apps/plugin-opener");
        await opener.openUrl(url);
        return { ok: true, value: null };
      } catch (error) {
        return failure("the browser did not open", error);
      }
    },

    async copyImage(png) {
      try {
        const clipboard = await import("@tauri-apps/plugin-clipboard-manager");
        const { Image: TauriImage } = await import("@tauri-apps/api/image");
        const image = await TauriImage.fromBytes(png);
        await clipboard.writeImage(image);
        return { ok: true, value: null };
      } catch (error) {
        return failure("the screenshot could not be copied", error);
      }
    },
  });
}
