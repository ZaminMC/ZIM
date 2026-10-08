// Servers store: the reconciled truth about registered servers. Seeded
// from `server.list` / the events snapshot, then kept live by
// `serverStateChanged` events (ADR-0006). The crash card carries the last
// crash classification per server until the next start.

import { create } from "zustand";
import type {
  CrashClassification,
  ProtocolErrorObject,
  ServerState,
  ServerSummary,
} from "../protocol/types";

/** What the sidebar and views know: a summary plus optional details. */
export type ServerEntry = ServerSummary & {
  software?: string;
  version?: string;
  port?: number;
};

export interface CrashInfo {
  serverId: string;
  phase: CrashClassification["phase"];
  exitCode?: number;
  evidence?: string;
  error?: ProtocolErrorObject;
  /** True once a later transition proves the crash card stale. */
  resolved: boolean;
}

interface ServersState {
  servers: Record<string, ServerEntry>;
  /** Load order: sidebar sorts by display name, not insertion. */
  replaceAll: (servers: ServerSummary[]) => void;
  /** Insert or refresh one server (e.g. after a `registered` event, which
   *  carries no display name — fetch details, then upsert). */
  upsert: (server: ServerSummary) => void;
  applyState: (serverId: string, state: ServerState) => void;
  crashes: Record<string, CrashInfo>;
  recordCrash: (crash: CrashInfo) => void;
  resolveCrash: (serverId: string) => void;
  forget: (serverId: string) => void;
}

export const useServers = create<ServersState>((set) => ({
  servers: {},
  crashes: {},

  replaceAll: (servers) =>
    set((state) => {
      const next: Record<string, ServerEntry> = {};
      for (const server of servers) next[server.serverId] = server;
      const crashes = { ...state.crashes };
      for (const serverId of Object.keys(crashes)) {
        const snapshotState = next[serverId]?.state;
        if (!(serverId in next) || (snapshotState !== undefined && snapshotState !== "crashed")) {
          // Absent from the registry, or the snapshot says it is no longer
          // crashed: the card is stale.
          delete crashes[serverId];
        }
      }
      return { servers: next, crashes };
    }),

  upsert: (server) =>
    set((state) => {
      const crashes = { ...state.crashes };
      const existing = crashes[server.serverId];
      // Details fetched after a register can carry a live state while a
      // stale crash card lingers: any state other than crashed proves
      // the card stale, exactly like applyState's rule (§62 keeps the
      // card only while the registry itself says crashed).
      if (existing && !existing.resolved && server.state !== "crashed") {
        crashes[server.serverId] = { ...existing, resolved: true };
      }
      return {
        servers: { ...state.servers, [server.serverId]: server },
        crashes,
      };
    }),

  applyState: (serverId, next) =>
    set((state) => {
      const current = state.servers[serverId];
      const crashes = { ...state.crashes };
      // A later transition on THIS server proves its crash card stale.
      if (next !== "crashed" && crashes[serverId]) {
        crashes[serverId] = { ...crashes[serverId], resolved: true };
      }
      if (!current) return { crashes };
      return {
        servers: { ...state.servers, [serverId]: { ...current, state: next } },
        crashes,
      };
    }),

  recordCrash: (crash) => set((state) => ({ crashes: { ...state.crashes, [crash.serverId]: crash } })),
  resolveCrash: (serverId) =>
    set((state) => {
      const crash = state.crashes[serverId];
      if (!crash) return state;
      return { crashes: { ...state.crashes, [serverId]: { ...crash, resolved: true } } };
    }),

  forget: (serverId) =>
    set((state) => {
      const servers = { ...state.servers };
      const crashes = { ...state.crashes };
      delete servers[serverId];
      delete crashes[serverId];
      return { servers, crashes };
    }),
}));

/** Sorted view for the sidebar. */
export function sortedServers(servers: Record<string, ServerEntry>): ServerEntry[] {
  return Object.values(servers).sort((a, b) => a.displayName.localeCompare(b.displayName));
}
