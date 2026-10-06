// Connection store: mirrors the protocol client's status for the UI.
// The client is the single source of truth; this is a projection.

import { create } from "zustand";
import type { ProtocolClient } from "../protocol/client";

export type ConnectionStatus = "offline" | "connecting" | "ready";

export interface DaemonInfo {
  name: string;
  version: string;
}

interface ConnectionState {
  status: ConnectionStatus;
  daemon: DaemonInfo | null;
  lastError: string | null;
  bind: (client: ProtocolClient) => () => void;
  setReady: (daemon: DaemonInfo) => void;
  setOffline: (error: string | null) => void;
}

export const useConnection = create<ConnectionState>((set) => ({
  status: "offline",
  daemon: null,
  lastError: null,

  bind: (client) => {
    const unbind = client.onStatus((status) => {
      if (status === "ready") {
        const info = client.daemonInfo;
        set({
          status,
          daemon: info ? { name: info.daemon.name, version: info.daemon.version } : null,
          lastError: null,
        });
      } else {
        const failure = client.lastError;
        set({
          status,
          lastError: failure instanceof Error ? failure.message : null,
        });
      }
    });
    return unbind;
  },

  setReady: (daemon) => set({ status: "ready", daemon, lastError: null }),
  setOffline: (error) => set({ status: "offline", lastError: error }),
}));
