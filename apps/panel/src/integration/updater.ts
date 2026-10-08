// The update lane's desktop seam (ADR-0024): the webview owns every
// decision — when to check, whether "automatic" applies, when to restart —
// and this module is the only place that talks to the desktop plugins that
// perform the OS calls. Development runs in a plain browser against the dev
// bridge, so the real backend resolves to null there; call sites decide what
// "unavailable" looks like (STYLE-GUIDE: honest states, never a fake
// success).
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
  /** Download and install the offer the last check returned. */
  install(): Promise<UpdateResult<null>>;
  /** Restart the app into the installed version. */
  relaunch(): Promise<UpdateResult<null>>;
}

function failure(what: string, error: unknown): { ok: false; message: string } {
  const message = error instanceof Error ? error.message : String(error);
  return { ok: false, message: `${what}: ${message}` };
}

// The updater plugin hands back a live object from check(); its install()
// must be called on THAT object, so the real backend keeps the one pending
// offer in its own slot — the store sees plain data only.
type PluginUpdate = {
  version: string;
  body?: string;
  date?: string;
  downloadAndInstall?: (onEvent?: (event: unknown) => void) => Promise<void>;
};

let pendingOffer: PluginUpdate | null = null;

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
        pendingOffer = null;
        return failure("the update check failed", error);
      }
    },

    async install() {
      const update = pendingOffer;
      if (!update?.downloadAndInstall) {
        return { ok: false, message: "No update is pending — check for updates first." };
      }
      try {
        await update.downloadAndInstall();
        return { ok: true, value: null };
      } catch (error) {
        return failure("the update install failed", error);
      }
    },

    async relaunch() {
      try {
        const plugin = await import("@tauri-apps/plugin-process");
        await plugin.relaunch();
        // A successful relaunch never returns; reaching this line means the
        // OS refused the restart.
        return { ok: false, message: "The restart did not happen — relaunch manually." };
      } catch (error) {
        return failure("the restart failed", error);
      }
    },
  });
}
