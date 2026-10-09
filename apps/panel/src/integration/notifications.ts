// Desktop notifications (Phase 7): the taxonomy lives here, the OS call
// rides the host's notification plugin. The rule is operator empathy —
// a notification is for what the operator must know *while not looking*.
// The focused window already has in-app surfaces for every event here
// (crash card, job progress chips, action errors), so notifications fire
// only when the window is hidden or unfocused. In a plain browser
// (development) nothing is ever sent.

import { isTauri } from "./tauri";

export interface NotificationSpec {
  title: string;
  body: string;
}

/** The focus shape decisions take, so tests never fake global state. */
export interface Focus {
  /** document.hidden — minimized, other tab, closed into the tray. */
  hidden: boolean;
  /** !document.hasFocus() — visible but not the front window. */
  blurred: boolean;
}

export function isUnfocused(): Focus {
  if (typeof document === "undefined") return { hidden: true, blurred: true };
  return { hidden: document.hidden, blurred: !document.hasFocus() };
}

/** Notifications matter only when the panel is not the operator's focus. */
export function shouldNotify(focus: Focus): boolean {
  return focus.hidden || focus.blurred;
}

// --- taxonomy (pure, tested) -------------------------------------------------

const crashPhase = (phase: string): string =>
  phase === "startup" ? "during startup" : phase === "runtime" ? "while running" : phase;

export function crashNotification(server: {
  displayName: string;
  exitCode?: number;
  phase: string;
}): NotificationSpec {
  const why = crashPhase(server.phase);
  const exit =
    server.exitCode === undefined ? "" : ` (exit code ${server.exitCode})`;
  return {
    title: `${server.displayName} crashed`,
    body: `The server went down ${why}${exit}. Open ZIM for the evidence and recovery options.`,
  };
}

export type JobOutcomeName = "succeeded" | "failed" | "cancelled";

const jobKindTitle = (kind: string): string => {
  switch (kind) {
    case "server.create":
      return "Server creation";
    case "backup.create":
      return "Backup";
    case "backup.restore":
      return "Restore";
    case "archive.extract":
      return "Archive extraction";
    case "java.install":
      return "Java download";
    default:
      return "Job";
  }
};

export function jobNotification(job: {
  kind: string;
  outcome: JobOutcomeName;
  serverName?: string;
}): NotificationSpec {
  const what = job.serverName
    ? `${jobKindTitle(job.kind)} — ${job.serverName}`
    : jobKindTitle(job.kind);
  switch (job.outcome) {
    case "succeeded":
      return { title: `${what} finished`, body: `${what} completed successfully.` };
    case "cancelled":
      return { title: `${what} cancelled`, body: `${what} was cancelled before it finished.` };
    case "failed":
      return {
        title: `${what} failed`,
        body: `${what} failed. Open ZIM for the typed error and what to do next.`,
      };
  }
}

// --- delivery (Tauri-only, never throws) --------------------------------------

let permissionGranted: boolean | null = null;

async function ensurePermission(): Promise<boolean> {
  if (!isTauri) return false;
  if (permissionGranted !== null) return permissionGranted;
  try {
    const plugin = await import("@tauri-apps/plugin-notification");
    let granted = await plugin.isPermissionGranted();
    if (!granted) granted = (await plugin.requestPermission()) === "granted";
    permissionGranted = granted;
  } catch {
    permissionGranted = false;
  }
  return permissionGranted;
}

/** Fire-and-forget delivery. Never rejects, never logs past a debug line. */
export async function deliver(spec: NotificationSpec): Promise<void> {
  if (!(await ensurePermission())) return;
  try {
    const plugin = await import("@tauri-apps/plugin-notification");
    plugin.sendNotification({ title: spec.title, body: spec.body });
  } catch {
    // A toast must never take the wire down.
  }
}
