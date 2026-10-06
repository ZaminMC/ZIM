// Lifecycle state → color + pulse. Reads the ADR-0005 states.

import styles from "./StatusDot.module.css";
import type { ServerState } from "../protocol/types";

const liveStates = new Set<ServerState>(["starting", "stopping", "adopting"]);

export function stateColor(state: ServerState): string {
  const key = `--state-${state}`;
  const value = getComputedStyle(document.documentElement).getPropertyValue(key).trim();
  return value || "var(--state-unknown)";
}

export function StatusDot({ state }: { state: ServerState }) {
  const live = liveStates.has(state);
  let classes: string = styles.dot ?? "";
  if (live) classes += ` ${styles.live ?? ""}`;
  return (
    <span
      className={classes}
      style={{ background: stateColor(state), color: stateColor(state) }}
      title={state}
      aria-label={`state: ${state}`}
    />
  );
}
