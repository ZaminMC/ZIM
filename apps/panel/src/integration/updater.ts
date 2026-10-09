// The update lane's desktop seam (ADR-0024): the webview owns every
// decision — when to check, whether "automatic" applies, when to restart —
// and this module is the only place that talks to the desktop plugins that
// perform the OS calls. Development runs in a plain browser against the dev
// bridge, so the real backend resolves to null there; call sites decide what
// "unavailable" looks like (STYLE-GUIDE: honest states, never a fake
// success).
//
// The one seam law that keeps the lane seamless (§82): DOWNLOAD and APPLY
// are two different operations with two different blast radii.
//   • download() only fetches the package — on every platform it is safe to
//     run at any moment, and nothing on screen changes ownership.
//   • applyAndRestart() is where Windows hands the process over: the plugin
//     launches the signed NSIS installer (passive progress bar), the
//     installer uninstalls the current build, installs the new one,
//     relaunches, and the plugin ends this process with exit(0) before the
//     call resolves. That is why apply is NEVER automatic — it must sit
//     behind an explicit operator click, or the panel would close itself
//     mid-work the moment a release lands on the channel.
//
// Every method answers a Result instead of throwing: an update lane that
// fails must be a visible state (§81), never a silent one and never a
// crash.

import { isTauri } from "./tauri";

/** What the channel offers when the installed version is behind. */
export interface UpdateOffer {
  version: string;
  /** Release notes as plain text (the manifest's `notes`). */
  notes?: string;
  /** ISO 8601 publication date from the manifest. */
  pubDate?: string;
}

export type UpdateResult<T> = { ok: true; value: T } | { ok: false; message: string };

/** The update lane's capability surface — injected, so the store's
 *  decisions are testable without a desktop host. */
export interface UpdateBackend {
  /** The installed app's version — null where there is none. */
  currentVersion(): Promise<UpdateResult<string | null>>;
  /** Ask the channel: an offer, null when this version is the newest. */
  check(): Promise<UpdateResult<UpdateOffer | null>>;
  /** Fetch the update package. Safe everywhere; nothing applies yet. */
  download(): Promise<UpdateResult<null>>;
  /** Apply the downloaded package and restart into it. On Windows this
   *  ends the process inside the call (the installer relaunches itself);
   *  on macOS/Linux the panel falls back to its own relaunch. */
  applyAndRestart(): Promise<UpdateResult<null>>;
}

function failure(what: string, error: unknown): { ok: false; message: string } {
  const message = error instanceof Error ? error.message : String(error);
  return { ok: false, message: `${what}: ${message}` };
}

// The updater plugin hands back a live object from check(); its download()
// and install() must be called on THAT object, so the real backend keeps the
// one pending offer in its own slot — the store sees plain data only.
type PluginUpdate = {
  version: string;
  body?: string;
  date?: string;
  download?: (onEvent?: (event: unknown) => void) => Promise<void>;
  install?: (options?: { restartAfterInstall?: boolean }) => Promise<void>;
  /** The resource's own release (the Rust side holds the offer's bytes). */
  close?: () => Promise<void>;
};

let pendingOffer: PluginUpdate | null = null;
/** Whether the pending offer's package has been fetched (its resource holds
 *  the bytes Rust-side; closing a downloaded offer would throw them away). */
let offerDownloaded = false;

/** Drop a superseded offer's Rust-side resource — but never a downloaded
 *  one, whose bytes are the reason the next restart can apply offline. */
function discardPending(): void {
  if (pendingOffer && !offerDownloaded) void pendingOffer.close?.();
  pendingOffer = null;
  offerDownloaded = false;
}

export function realUpdateBackend(): Promise<UpdateBackend | null> {
  if (!isTauri) return Promise.resolve(null);
  return Promise.resolve({
    async currentVersion() {
      try {
        const app = await import("@tauri-apps/api/app");
        return { ok: true, value: await app.getVersion() };
      } catch (error) {
        return failure("the installed version is unknown", error);
      }
    },

    async check() {
      try {
        const plugin = await import("@tauri-apps/plugin-updater");
        const update = await plugin.check();
        // A fresh answer replaces the old offer; the old resource is only
        // released when nothing was downloaded into it.
        discardPending();
        pendingOffer = update ?? null;
        if (!update) return { ok: true, value: null };
        return {
          ok: true,
          value: {
            version: update.version,
            notes: update.body,
            pubDate: update.date,
          },
        };
      } catch (error) {
        discardPending();
        return failure("the update check failed", error);
      }
    },

    async download() {
      const update = pendingOffer;
      if (!update?.download) {
        return { ok: false, message: "No update is pending — check for updates first." };
      }
      try {
        await update.download();
        offerDownloaded = true;
        return { ok: true, value: null };
      } catch (error) {
        return failure("the update download failed", error);
      }
    },

    async applyAndRestart() {
      const update = pendingOffer;
      if (!update?.install) {
        return { ok: false, message: "No update is pending — check for updates first." };
      }
      try {
        // Windows: the plugin spawns the signed installer (passive), which
        // uninstalls the current build, installs the new one, relaunches,
        // and then ends this process — the promise below never resolves.
        await update.install({ restartAfterInstall: true });
        // macOS/Linux: the package is applied in place; the panel still
        // runs the old binary, so the restart is the panel's own move.
        const process = await import("@tauri-apps/plugin-process");
        await process.relaunch();
        // A successful relaunch never returns; reaching this line means the
        // OS refused the restart.
        return { ok: false, message: "The restart did not happen — relaunch manually." };
      } catch (error) {
        return failure("the update install failed", error);
      }
    },
  });
}
