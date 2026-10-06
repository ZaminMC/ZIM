// Status chip: the lifecycle state as a labeled pill (dot + word). The
// bare StatusDot stays the space-efficient mark for tight rows (sidebar);
// headers, cards, and dashboards use this labeled form.

import { stateColor } from "./StatusDot";
import type { ServerState } from "../protocol/types";
import styles from "./StatusChip.module.css";

const LABELS: Record<string, string> = {
  running: "Running",
  starting: "Starting",
  stopping: "Stopping",
  stopped: "Stopped",
  "not-running": "Not running",
  "failed-preflight": "Preflight failed",
  crashed: "Crashed",
  adopting: "Adopting",
  unknown: "Unknown",
};

export function StatusChip({ state }: { state: ServerState }) {
  const live = state === "starting" || state === "stopping" || state === "adopting";
  return (
    <span
      className={[styles.chip, live ? styles.live : ""].filter(Boolean).join(" ")}
      style={{
        color: stateColor(state),
        background: `color-mix(in srgb, ${stateColor(state)} 10%, transparent)`,
        borderColor: `color-mix(in srgb, ${stateColor(state)} 30%, transparent)`,
      }}
    >
      <span className={styles.dot} style={{ background: stateColor(state) }} aria-hidden />
      {LABELS[state] ?? state}
    </span>
  );
}
