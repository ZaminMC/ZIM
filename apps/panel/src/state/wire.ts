// The wire: one client instance for the app, bound to the stores. In a
// Tauri webview it rides the host bridge; in a plain browser (development)
// it rides the WebSocket dev bridge. Everything above this module is
// transport-agnostic.

import { ProtocolClient } from "../protocol/client";
import { TauriTransport, WsTransport } from "../protocol/transport";
import type { Transport } from "../protocol/transport";
import { logWarn } from "../logger";
import { useConnection } from "./connection";
import { useServers } from "./servers";
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

async function wireEvents(): Promise<void> {
  await client.subscribe("events", undefined, {
    onPayload: (notification) => {
      if (notification.payload.kind !== "event") return;
      const event = notification.payload.event;
      if (event.type !== "serverStateChanged") return;

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
