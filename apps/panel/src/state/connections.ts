// Connection profiles (ADR-0011): which daemon this panel talks to.
// PANEL-LOCAL storage, never daemon state — persisted to localStorage.
// The local profile is implicit and always present; remote profiles carry
// the agent's address, the token (sent as hello.auth over TLS), and the
// pinned certificate fingerprint.

import { create } from "zustand";
import { persist } from "zustand/middleware";

export interface RemoteProfile {
  id: string;
  name: string;
  /** `host:port` of the agent. */
  addr: string;
  token: string;
  /** SHA-256 hex of the agent certificate; empty means skip-verify (the
   *  documented, discouraged escape hatch). */
  fingerprint: string;
}

export type ConnectionProfile =
  | { id: "local"; name: string }
  | RemoteProfile;

export const LOCAL_PROFILE: ConnectionProfile = { id: "local", name: "This machine" };

const STORAGE_KEY = "zamin.connections";

interface ConnectionsState {
  remotes: RemoteProfile[];
  activeId: string;
  addRemote: (profile: Omit<RemoteProfile, "id">) => string;
  updateRemote: (id: string, patch: Partial<Omit<RemoteProfile, "id">>) => void;
  removeRemote: (id: string) => void;
  setActive: (id: string) => void;
}

export const useConnections = create<ConnectionsState>()(
  persist(
    (set) => ({
      remotes: [],
      activeId: LOCAL_PROFILE.id,

      addRemote: (profile) => {
        const id =
          typeof crypto !== "undefined" && "randomUUID" in crypto
            ? crypto.randomUUID()
            : `remote-${Date.now()}`;
        set((state) => ({ remotes: [...state.remotes, { ...profile, id }] }));
        return id;
      },

      updateRemote: (id, patch) =>
        set((state) => ({
          remotes: state.remotes.map((r) => (r.id === id ? { ...r, ...patch } : r)),
        })),

      removeRemote: (id) =>
        set((state) => ({
          remotes: state.remotes.filter((r) => r.id !== id),
          activeId: state.activeId === id ? LOCAL_PROFILE.id : state.activeId,
        })),

      setActive: (id) => set({ activeId: id }),
    }),
    { name: STORAGE_KEY },
  ),
);

/** The active profile, materialized (the local profile is not stored). */
export function activeProfile(state: {
  remotes: RemoteProfile[];
  activeId: string;
}): ConnectionProfile {
  return (
    state.remotes.find((r) => r.id === state.activeId) ?? LOCAL_PROFILE
  );
}

/** Transport spec for the active profile: the WebSocket URL the dev bridge
 *  relays from, plus the hello credential. Browser/dev path only — the
 *  Tauri host speaks to the agent from Rust (ADR-0011). */
export function transportSpec(
  bridgeUrl: string,
  state: { remotes: RemoteProfile[]; activeId: string },
): { url: string; auth: string | undefined } {
  const profile = activeProfile(state);
  // `id: "local"` is not an exclusive discriminant (RemoteProfile.id is a
  // plain string), so the shape decides.
  if (!("addr" in profile)) {
    return { url: bridgeUrl, auth: undefined };
  }
  const query = new URLSearchParams({
    remote: profile.addr,
    ...(profile.fingerprint ? { fingerprint: profile.fingerprint } : {}),
  });
  return { url: `${bridgeUrl}/?${query.toString()}`, auth: profile.token };
}
