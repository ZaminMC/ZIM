// The wire: one client instance for the app, bound to the stores. In a
// Tauri webview it rides the host bridge; in a plain browser (development)
// it rides the WebSocket dev bridge. Everything above this module is
// transport-agnostic.

import { ProtocolClient } from "../protocol/client";
import { TauriTransport, WsTransport } from "../protocol/transport";
import type { Transport } from "../protocol/transport";
import {
  crashNotification,
  deliver,
  isUnfocused,
  jobNotification,
  shouldNotify,
} from "../integration/notifications";
import type { JobOutcomeName } from "../integration/notifications";
import { logWarn } from "../logger";
import { useConnection } from "./connection";
import { useServers } from "./servers";
import { useJobs } from "./jobs";
import { getServer } from "./actions";

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const bridgeUrl = import.meta.env.VITE_BRIDGE_URL ?? "ws://127.0.0.1:8787";

export const client = new ProtocolClient((): Promise<Transport> =>
  isTauri ? Promise.resolve(new TauriTransport()) : Promise.resolve(new WsTransport(bridgeUrl)),
);

if (import.meta.env.DEV) {
  // Dev diagnostics hook: browser-console access to the live client.
  (window as unknown as { __zaminClient?: ProtocolClient }).__zaminClient = client;
}

let started = false;

/** Idempotent: bind stores, connect, and keep the events stream open. */
export function startWire(): void {
  if (started) return;
  started = true;

  useConnection.getState().bind(client);
  void client.connect().catch(() => {}); // failures schedule their own retries
  void wireEvents();
}

// --- notifications (Phase 7 taxonomy: only while the operator is away) ---

function serverName(serverId: string): string {
  return useServers.getState().servers[serverId]?.displayName ?? serverId;
}

function notifyCrash(serverId: string, phase: string, exitCode?: number): void {
  const focus = isUnfocused();
  if (!shouldNotify(focus)) return;
  void deliver(
    crashNotification({
      displayName: serverName(serverId),
      phase,
      exitCode,
    }),
  );
}

function notifyJob(jobId: string, outcome: JobOutcomeName): void {
  const focus = isUnfocused();
  if (!shouldNotify(focus)) return;
  const job = useJobs.getState().jobs[jobId];
  if (!job) return;
  void deliver(
    jobNotification({
      kind: job.kind,
      outcome,
      serverName: job.serverId === undefined ? undefined : serverName(job.serverId),
    }),
  );
}

async function wireEvents(): Promise<void> {
  await client.subscribe("events", undefined, {
    onPayload: (notification) => {
      if (notification.payload.kind !== "event") return;
      const event = notification.payload.event;
      if (event.type !== "serverStateChanged" && !event.type.startsWith("job")) return;

      if (event.type === "jobStarted") {
        useJobs.getState().started(event.job);
        return;
      }
      if (event.type === "jobProgress") {
        useJobs.getState().progress(event.jobId, event.progress);
        return;
      }
      if (event.type === "jobCompleted") {
        useJobs.getState().completed(event.jobId, event.outcome, event.error);
        notifyJob(event.jobId, event.outcome);
        return;
      }

      const state = useServers.getState();
      if (event.reason === "removed") {
        state.forget(event.serverId);
        return;
      }
      state.applyState(event.serverId, event.to);
      if (event.reason === "registered") {
        // The event carries no display name; fetch details and upsert.
        void getServer(event.serverId)
          .then((details) => useServers.getState().upsert(details))
          .catch((error: unknown) => logWarn("wire", "server.get after register failed", error));
      }
      if (event.to === "crashed" && event.crash) {
        state.recordCrash({
          serverId: event.serverId,
          phase: event.crash.phase,
          exitCode: event.crash.exitCode,
          evidence: event.crash.evidence,
          error: event.error,
          resolved: false,
        });
        notifyCrash(event.serverId, event.crash.phase, event.crash.exitCode);
      }
    },
    onRegistered: (result) => {
      if (result.snapshot) {
        useServers.getState().replaceAll(result.snapshot.servers);
      }
    },
  }).catch((error: unknown) => {
    logWarn("wire", "events subscription failed", error);
  });
}
