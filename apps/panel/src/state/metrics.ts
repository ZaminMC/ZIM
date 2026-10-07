// Live per-server metrics (ADR-0006): one metrics subscription per OPEN
// server workspace, reference-counted so the header chips and the Metrics
// tab share it. Samples land in a bounded window mirroring the daemon's
// ring (600 @ 1 Hz); duplicates (ring replay vs. live tick) are dropped by
// timestamp so a resubscription never double-plots a second.

import { create } from "zustand";
import { metricsRange, subscribeMetrics } from "./actions";
import { logWarn } from "../logger";
import type { MetricsSample } from "../protocol/types";

/** Mirrors the daemon's METRICS_RING: 600 one-second samples. */
export const METRICS_WINDOW = 600;

interface MetricsState {
  /** Per-server history, chronological (oldest first), newest last. */
  samples: Record<string, MetricsSample[]>;
  append: (serverId: string, sample: MetricsSample) => void;
  seed: (serverId: string, history: MetricsSample[]) => void;
  forget: (serverId: string) => void;
}

export const useMetrics = create<MetricsState>()((set) => ({
  samples: {},
  append: (serverId, sample) =>
    set((state) => {
      const history = state.samples[serverId] ?? [];
      const last = history.at(-1);
      // The fresh-subscription replay and the live tick can both deliver
      // the same (or an older ring) sample; timestamps are the dedup key.
      if (last && sample.tsMs <= last.tsMs) return state;
      const next = [...history, sample];
      if (next.length > METRICS_WINDOW) next.splice(0, next.length - METRICS_WINDOW);
      return { samples: { ...state.samples, [serverId]: next } };
    }),
  seed: (serverId, history) =>
    set((state) => {
      if (history.length === 0) return state;
      const existing = state.samples[serverId] ?? [];
      // Merge by timestamp: range history first, keep anything live that is
      // newer than the last seeded sample.
      const lastSeeded = history.at(-1)?.tsMs ?? 0;
      const merged = [
        ...history,
        ...existing.filter((sample) => sample.tsMs > lastSeeded),
      ].slice(-METRICS_WINDOW);
      return { samples: { ...state.samples, [serverId]: merged } };
    }),
  forget: (serverId) =>
    set((state) => {
      if (!(serverId in state.samples)) return state;
      const next = { ...state.samples };
      delete next[serverId];
      return { samples: next };
    }),
}));

// Reference-counted subscriptions live outside the store: they are wiring,
// not state, and StrictMode double-mounts must not double-subscribe.
const counts = new Map<string, number>();
const disposers = new Map<string, () => void>();

/** Begin (or join) the metrics stream for a server. Fire-and-forget: the
 *  store simply stays empty until the wire is up — no error surfaces from
 *  a background stream. */
export function ensureMetrics(serverId: string): void {
  const count = (counts.get(serverId) ?? 0) + 1;
  counts.set(serverId, count);
  if (count > 1) return;

  // History first, then the live stream (its ring replay is deduped).
  void metricsRange({ serverId, maxSamples: METRICS_WINDOW })
    .then((result) => useMetrics.getState().seed(serverId, result.samples))
    .catch(() => {}); // unknown server or wire down: live samples still flow
  void subscribeMetrics(serverId, {
    onPayload: (notification) => {
      if (notification.payload.kind === "metrics") {
        useMetrics.getState().append(serverId, notification.payload.sample);
      }
    },
  })
    .then((sub) => {
      if ((counts.get(serverId) ?? 0) < 1) {
        // Unmounted while the subscribe was in flight.
        sub.dispose();
        return;
      }
      disposers.set(serverId, () => sub.dispose());
    })
    .catch((error: unknown) => logWarn("metrics", "metrics stream failed", error));
}

/** Drop one reference; the last one closes the subscription. History
 *  stays in the store (bounded) so re-opening a server is instant. */
export function releaseMetrics(serverId: string): void {
  const count = (counts.get(serverId) ?? 1) - 1;
  if (count > 0) {
    counts.set(serverId, count);
    return;
  }
  counts.delete(serverId);
  disposers.get(serverId)?.();
  disposers.delete(serverId);
}

/** A server is gone from the registry: drop its history entirely. */
export function forgetMetrics(serverId: string): void {
  useMetrics.getState().forget(serverId);
}

export function useServerMetrics(serverId: string): MetricsSample[] {
  return useMetrics((s) => s.samples[serverId]) ?? [];
}

// --- formatting helpers shared by the chips and the metrics view ---

export function formatBytes(bytes?: number): string {
  if (bytes === undefined || Number.isNaN(bytes)) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 100 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

export function formatCpu(percent?: number): string {
  if (percent === undefined || Number.isNaN(percent)) return "—";
  return `${percent >= 100 ? Math.round(percent) : percent.toFixed(1)}%`;
}

export function formatUptime(ms?: number): string {
  if (ms === undefined || ms < 0) return "—";
  const totalSeconds = Math.floor(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`;
}
