// The update lane's decisions (ADR-0024): when the panel asks the channel,
// what "automatic" means, and when a restart happens. The desktop seam is
// injected (`setUpdateBackend`), so every decision here is testable in a
// plain browser and the lane degrades honestly to "unavailable" in dev.
//
// The rules this store follows:
//   • §82 — no fake states: "ready" means the package is downloaded and
//     verified; applying it is the restart, and the restart is the
//     operator's own click — never forced mid-work (on Windows the apply
//     ends the process, see integration/updater.ts). Nothing is persisted
//     about the lane's runtime phase — a fresh boot is idle.
//   • §81 — failures are human sentences carrying the technical message,
//     never a bare code.
//   • Automatic means download; the apply waits for the explicit restart.

import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import type { UpdateBackend, UpdateOffer } from "../integration/updater";
import { realUpdateBackend } from "../integration/updater";

/** The lane's honest phases — the frame banner and the settings rows render
 *  exactly what these say, nothing more. */
export type UpdatePhase =
  | { kind: "unavailable" } // browser dev: no desktop host, no lane
  | { kind: "idle" } // desktop: the lane exists, no answer yet
  | { kind: "checking" }
  | { kind: "upToDate"; version: string }
  | { kind: "available"; offer: UpdateOffer } // waiting on the operator (auto-download off)
  | { kind: "downloading"; offer: UpdateOffer } // the package fetch runs under this phase
  | { kind: "ready"; offer: UpdateOffer } // downloaded; the apply is the restart
  | { kind: "error"; message: string };

export interface UpdatesPrefs {
  /** Ask the channel on boot and every six hours. */
  autoCheck: boolean;
  /** Download an offered update without asking (the apply stays manual). */
  autoInstall: boolean;
}

interface UpdatesState {
  phase: UpdatePhase;
  prefs: UpdatesPrefs;
  /** The installed version, once the host could answer — runtime only. */
  installedVersion: string | null;
  setAutoCheck: (on: boolean) => void;
  setAutoInstall: (on: boolean) => void;
  /** Ask the channel now. A manual check runs even when auto-check is off. */
  check: (manual: boolean) => Promise<void>;
  /** Download the pending offer when auto-download is off. */
  installNow: () => Promise<void>;
  /** Apply the downloaded update and restart (only honest from "ready"). */
  restart: () => Promise<void>;
  /** Acknowledge an error / the "up to date" answer — back to idle. */
  dismiss: () => void;
}

export const AUTO_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;
export const FIRST_CHECK_DELAY_MS = 8_000;

let backend: UpdateBackend | null = null;
/** The offer the last successful check returned, mirrored from the seam. */
let pendingOffer: UpdateOffer | null = null;
let checkInFlight = false;

/** Test seam: inject a fake backend (null = the browser-dev shape). */
export function setUpdateBackend(injected: UpdateBackend | null): void {
  backend = injected;
  pendingOffer = null;
}

// --- scheduling (module-level, so tests can start/stop deterministically) ---

let timer: ReturnType<typeof setTimeout> | null = null;

function scheduleNextCheck(delay: number): void {
  if (timer !== null) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = null;
    void useUpdates.getState().check(false);
  }, delay);
}

function cancelScheduledCheck(): void {
  if (timer !== null) {
    clearTimeout(timer);
    timer = null;
  }
}

/** Boot the lane: resolve the real backend, learn the installed version,
 *  and start the auto-check cadence. Idempotent — the App calls it once,
 *  and a StrictMode double-mount must not double-schedule. */
export async function startUpdates(): Promise<void> {
  if (backend === null) backend = await realUpdateBackend();
  if (backend === null) {
    if (useUpdates.getState().phase.kind === "idle") {
      useUpdates.setState({ phase: { kind: "unavailable" } });
    }
    return;
  }
  const version = await backend.currentVersion();
  if (version.ok) useUpdates.setState({ installedVersion: version.value });
  if (useUpdates.getState().phase.kind === "idle" && useUpdates.getState().prefs.autoCheck) {
    scheduleNextCheck(FIRST_CHECK_DELAY_MS);
  }
}

/** Stop the cadence (tests, teardown). The phase is left as it is. */
export function stopUpdates(): void {
  cancelScheduledCheck();
}

// --- the copy (§81: every state is a human sentence) -------------------------

/** The one sentence a phase renders. The frame banner shows only what the
 *  operator must act on; the Settings updates rows show every phase —
 *  including the quiet ones. */
export function updatesSentence(phase: UpdatePhase): string {
  switch (phase.kind) {
    case "unavailable":
      return "The update lane lives in the desktop build — browser development has none.";
    case "idle":
      return "Not checked yet.";
    case "checking":
      return "Asking the update channel…";
    case "upToDate":
      return `ZIM ${phase.version} is the newest development build.`;
    case "available":
      return `ZIM ${phase.offer.version} is available.`;
    case "downloading":
      return `ZIM ${phase.offer.version} is downloading — nothing applies until you restart.`;
    case "ready":
      return `ZIM ${phase.offer.version} is ready — restart to apply it.`;
    case "error":
      return phase.message;
  }
}

// --- the store ---------------------------------------------------------------

/** A new check must not stomp work already in flight, and a pending
 *  restart must not be quietly forgotten by a fresh answer. */
function checkAllowed(manual: boolean): boolean {
  const phase = useUpdates.getState().phase;
  if (phase.kind === "downloading") return false;
  if (phase.kind === "ready") return false; // the settings row explains why
  if (checkInFlight) return false;
  if (!manual && !useUpdates.getState().prefs.autoCheck) return false;
  return true;
}

export const useUpdates = create<UpdatesState>()(
  persist(
    (set, get) => {
      async function installOffer(): Promise<void> {
        const offer = pendingOffer;
        if (backend === null || !offer) return;
        set({ phase: { kind: "downloading", offer } });
        const downloaded = await backend.download();
        if (!downloaded.ok) {
          set({ phase: { kind: "error", message: downloaded.message } });
          return;
        }
        set({ phase: { kind: "ready", offer } });
      }

      return {
        phase: { kind: "idle" },
        prefs: { autoCheck: true, autoInstall: true },
        installedVersion: null,

        setAutoCheck: (on) => {
          const state = get();
          set({ prefs: { ...state.prefs, autoCheck: on } });
          if (on) {
            // Re-arm from any quiet phase (idle, up-to-date, error, a
            // stale unavailable) — busy phases wait for the operator.
            const kind = state.phase.kind;
            if (kind === "idle" || kind === "upToDate" || kind === "error" || kind === "unavailable") {
              scheduleNextCheck(FIRST_CHECK_DELAY_MS);
            }
          } else {
            cancelScheduledCheck();
          }
        },

        setAutoInstall: (on) =>
          set((state) => ({ prefs: { ...state.prefs, autoInstall: on } })),

        check: async (manual) => {
          if (!checkAllowed(manual)) return;
          if (backend === null) {
            if (manual) set({ phase: { kind: "unavailable" } });
            return;
          }

          checkInFlight = true;
          set({ phase: { kind: "checking" } });
          const result = await backend.check();
          checkInFlight = false;

          if (!result.ok) {
            set({ phase: { kind: "error", message: result.message } });
            return;
          }
          if (result.value === null) {
            const version = get().installedVersion ?? "current";
            set({ phase: { kind: "upToDate", version } });
          } else {
            pendingOffer = result.value;
            if (get().prefs.autoInstall) {
              await installOffer();
            } else {
              set({ phase: { kind: "available", offer: result.value } });
            }
          }
          // The cadence continues only when it was ever on.
          if (get().prefs.autoCheck) scheduleNextCheck(AUTO_CHECK_INTERVAL_MS);
        },

        installNow: async () => {
          if (backend === null || pendingOffer === null) return;
          if (get().phase.kind !== "available") return;
          await installOffer();
        },

        restart: async () => {
          if (backend === null || get().phase.kind !== "ready") return;
          // Windows: the apply ends the process inside this call and the
          // installer relaunches the new build — this promise never lands.
          const applied = await backend.applyAndRestart();
          if (!applied.ok) {
            set({ phase: { kind: "error", message: applied.message } });
          }
        },

        dismiss: () => {
          const kind = get().phase.kind;
          if (kind === "error" || kind === "upToDate") set({ phase: { kind: "idle" } });
        },
      };
    },
    {
      name: "zim.updates",
      storage: createJSONStorage(() => localStorage),
      version: 1,
      // Only the prefs ride storage; the merge below drops whatever the
      // storage happens to hold of the runtime phase (a persisted "ready"
      // would claim an install that did not survive the restart it asked
      // for — §82).
      merge: (persisted, current) => {
        const v = (persisted ?? {}) as { prefs?: unknown };
        const prefs =
          typeof v.prefs === "object" && v.prefs !== null
            ? (v.prefs as Partial<UpdatesPrefs>)
            : {};
        return {
          ...current,
          prefs: {
            autoCheck: typeof prefs.autoCheck === "boolean" ? prefs.autoCheck : true,
            autoInstall: typeof prefs.autoInstall === "boolean" ? prefs.autoInstall : true,
          },
        };
      },
    },
  ),
);
