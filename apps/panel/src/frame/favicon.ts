// The favicon lane: the frame's own minimal projection of the fleet.
//
// WHY the frame keeps a wire: the strip's glyphs need each server's
// software and liveness, and the frame webview owns no other source —
// the content webviews' stores are their own realms (one JS realm per
// webview; nothing crosses but the host's snapshots). This lane dials
// the daemon directly with the SAME connection profile the content
// rides (the persisted connections store, read straight from
// localStorage), then keeps the projection live from the events stream
// and one log subscription per RUNNING server (the console-severity
// input the dot's law consumes).
//
// Deliberately NOT the content's startWire: no notifications machinery,
// no job store — the frame paints dots, and the projection names the
// fleet for the omnibox's suggestions (fleetBrief). The rendering can
// be silenced by the settings toggle (faviconPrefs); the lane itself
// stays push-only after boot.

import { create } from "zustand";
import { ProtocolClient } from "../protocol/client";
import { TauriTransport } from "../protocol/transport";
import type { ServerState } from "../protocol/types";
import {
  faviconDot,
  reduceConsoleSeverity,
  softwareKey,
  type ConsoleSeverity,
  type FaviconDot,
  type SoftwareKey,
} from "./faviconLaw";
import { logWarn } from "../logger";

export interface FaviconEntry {
  software: SoftwareKey;
  state: ServerState;
  severity: ConsoleSeverity;
  /** The registry's display name and port (the summaries carry both) —
   *  the projection the omnibox's suggestions read. A server registered
   *  after the boot list arrives nameless and is named by its
   *  server.get fetch. */
  name?: string;
  port?: number;
}

interface FaviconState {
  servers: Record<string, FaviconEntry>;
  /** The lane followed the ACTIVE connection profile at dial time. */
  profileId: string;
}

const initial: FaviconState = { servers: {}, profileId: "local" };

export const useFavicons = create<FaviconState>(() => initial);

/** The read the strip's render consumes (the dot + glyph for a server
 *  tab, or null when the lane knows nothing — the plain glyph renders). */
export function faviconFor(
  state: FaviconState,
  serverId: string,
): { dot: FaviconDot; software: SoftwareKey } | null {
  const entry = state.servers[serverId];
  if (!entry) return null;
  return {
    dot: faviconDot(entry.state, entry.severity),
    software: entry.software,
  };
}

/** The frame's fleet projection — what the omnibox's suggest call
 *  passes to the host: the registry's names, ids, states, ports. */
export function fleetBrief(state: FaviconState): Array<{
  server_id: string;
  display_name: string;
  state: string;
  port?: number;
}> {
  return Object.entries(state.servers).map(([serverId, entry]) => ({
    server_id: serverId,
    display_name: entry.name ?? serverId,
    state: entry.state,
    ...(entry.port != null ? { port: entry.port } : {}),
  }));
}

// --- the lane ---------------------------------------------------------------

const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let client: ProtocolClient | null = null;
let started = false;
/** One dispose per running server's log subscription. */
const logDisposals = new Map<string, () => void>();

interface PersistedConnections {
  state?: {
    remotes?: Array<{
      id: string;
      addr: string;
      token: string;
      fingerprint: string;
    }>;
    activeId?: string;
  };
}

/** The active profile, read from the persisted connections store (zustand
 *  persist writes the same key every webview of the origin shares). */
function persistedProfile(): {
  activeId: string;
  remote: { addr: string; token: string; fingerprint: string } | null;
} {
  try {
    const raw = localStorage.getItem("zamin.connections");
    if (raw) {
      const parsed = JSON.parse(raw) as PersistedConnections;
      const activeId = parsed.state?.activeId ?? "local";
      const remote = parsed.state?.remotes?.find((r) => r.id === activeId);
      if (remote) {
        return {
          activeId,
          remote: {
            addr: remote.addr,
            token: remote.token,
            fingerprint: remote.fingerprint,
          },
        };
      }
      return { activeId, remote: null };
    }
  } catch {
    // An unreadable store rides the local profile.
  }
  return { activeId: "local", remote: null };
}

/** The severity input per running server: one live log subscription,
 *  reduced by the law. A server leaving `running` drops its feed. */
function syncLogSubscriptions(servers: Record<string, FaviconEntry>): void {
  if (!client) return;
  for (const [serverId, entry] of Object.entries(servers)) {
    const needed = entry.state === "running";
    const has = logDisposals.has(serverId);
    if (needed && !has) {
      const handle = client.subscribe(
        "logs",
        serverId,
        {
          onPayload: (notification) => {
            if (notification.payload.kind !== "logs") return;
            const current = useFavicons.getState().servers[serverId];
            if (!current) return;
            // The batch reduces in order — the LAST relevant line wins.
            let severity = current.severity;
            for (const line of notification.payload.batch) {
              severity = reduceConsoleSeverity(
                severity,
                line.level,
                current.state,
              );
            }
            if (severity !== current.severity) {
              useFavicons.setState((s) => ({
                servers: { ...s.servers, [serverId]: { ...current, severity } },
              }));
            }
          },
        },
        // No initial cursor: the ring replay would only re-derive the
        // same verdict; the live feed is the dynamic input.
        undefined,
      );
      void handle
        .then((sub) => {
          logDisposals.set(serverId, () => sub.dispose());
        })
        .catch((error: unknown) =>
          logWarn("favicon", "log subscription failed", error),
        );
    } else if (!needed && has) {
      logDisposals.get(serverId)?.();
      logDisposals.delete(serverId);
    }
  }
  // Subscriptions for servers the projection no longer knows.
  for (const [serverId, dispose] of logDisposals) {
    if (!servers[serverId]) {
      dispose();
      logDisposals.delete(serverId);
    }
  }
}

/** Fold a registry summary into the projection; fetch what the summary
 *  lacks (ServerSummary carries no software) once per unknown server. */
function foldState(
  serverId: string,
  state: ServerState,
  clientRef: ProtocolClient,
  brief?: { name?: string; port?: number },
): void {
  useFavicons.setState((s) => {
    const existing = s.servers[serverId];
    if (existing) {
      // The brief's facts ride on top (the boot list names the fleet;
      // the events only carry the state).
      return {
        servers: {
          ...s.servers,
          [serverId]: {
            ...existing,
            ...(brief?.name != null ? { name: brief.name } : {}),
            ...(brief?.port != null ? { port: brief.port } : {}),
          },
        },
      };
    }
    return {
      servers: {
        ...s.servers,
        [serverId]: {
          software: softwareKey(undefined),
          state,
          severity: "none",
          ...(brief?.name != null ? { name: brief.name } : {}),
          ...(brief?.port != null ? { port: brief.port } : {}),
        },
      },
    };
  });
  const existing = useFavicons.getState().servers[serverId];
  if (existing && (existing.software === "unknown" || existing.name == null)) {
    void clientRef
      .request<{
        software?: string;
        state: ServerState;
        displayName?: string;
        port?: number;
      }>("server.get", { serverId })
      .then((details) => {
        useFavicons.setState((s) => {
          const current = s.servers[serverId];
          if (!current) return s;
          return {
            servers: {
              ...s.servers,
              [serverId]: {
                ...current,
                software: softwareKey(details.software),
                ...(details.displayName != null ? { name: details.displayName } : {}),
                ...(details.port != null ? { port: details.port } : {}),
              },
            },
          };
        });
      })
      .catch(() => {
        // Unknown software keeps the stand-in glyph — honest, not broken.
      });
  }
  syncLogSubscriptions(useFavicons.getState().servers);
}

export function startFaviconLane(): void {
  if (started || !isTauri) return;
  started = true;

  const profile = persistedProfile();
  useFavicons.setState({ profileId: profile.activeId });
  const laneClient = new ProtocolClient(() => {
    // The frame rides the host's transport with the profile's own dial —
    // the same shape wire.ts's factory builds for the content webviews.
    const current = persistedProfile();
    laneClient.setAuth(current.remote?.token);
    return Promise.resolve(new TauriTransport(current.remote));
  });
  client = laneClient;

  void laneClient.connect().then(async () => {
    try {
      const list = await laneClient.request<{
        servers: Array<{
          serverId: string;
          state: ServerState;
          displayName?: string;
          port?: number;
        }>;
      }>("server.list");
      for (const server of list.servers) {
        foldState(server.serverId, server.state, laneClient, {
          name: server.displayName,
          port: server.port,
        });
      }
    } catch (error: unknown) {
      logWarn("favicon", "server.list failed", error);
    }
  });

  void laneClient
    .subscribe("events", undefined, {
      onPayload: (notification) => {
        if (notification.payload.kind !== "event") return;
        const event = notification.payload.event as {
          type: string;
          serverId?: string;
          to?: ServerState;
          reason?: string;
        };
        if (event.type !== "serverStateChanged" || !event.serverId || !event.to)
          return;
        if (event.reason === "removed") {
          useFavicons.setState((s) => {
            const servers = { ...s.servers };
            delete servers[event.serverId as string];
            return { servers };
          });
          logDisposals.get(event.serverId)?.();
          logDisposals.delete(event.serverId);
          return;
        }
        foldState(event.serverId, event.to, laneClient);
      },
      onRegistered: (result) => {
        if (!result.snapshot) return;
        for (const server of result.snapshot.servers) {
          foldState(server.serverId, server.state, laneClient);
        }
      },
    })
    .catch((error: unknown) => {
      logWarn("favicon", "events subscription failed", error);
    });

  // A profile switch in another webview lands as a storage event — the
  // lane re-dials so the dots follow the operator's active daemon.
  window.addEventListener("storage", (event) => {
    if (event.key !== "zamin.connections" || event.newValue == null) return;
    const next = persistedProfile();
    if (next.activeId === useFavicons.getState().profileId) return;
    useFavicons.setState({ profileId: next.activeId });
    for (const dispose of logDisposals.values()) dispose();
    logDisposals.clear();
    void laneClient
      .reconnect()
      .catch((error: unknown) =>
        logWarn("favicon", "profile switch reconnect failed", error),
      );
  });
}

/** The demo fixture's door: the browser fixture dresses a few servers so
 *  the strip's glyphs render their dots without a daemon. */
export function seedFaviconsForDemo(
  servers: Record<
    string,
    {
      software?: string;
      state: ServerState;
      severity?: ConsoleSeverity;
      name?: string;
      port?: number;
    }
  >,
): void {
  const next: Record<string, FaviconEntry> = {};
  for (const [serverId, server] of Object.entries(servers)) {
    next[serverId] = {
      software: softwareKey(server.software),
      state: server.state,
      severity: server.severity ?? "none",
      ...(server.name != null ? { name: server.name } : {}),
      ...(server.port != null ? { port: server.port } : {}),
    };
  }
  useFavicons.setState({ servers: next });
}
