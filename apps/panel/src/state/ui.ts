// View state (ADR-0003/ADR-0015): modal and operation surfaces are
// PANEL-LOCAL state, never daemon state. Navigation lives in the tabs
// store (state/tabs.ts) — this store keeps only what the frame shares:
// pending lifecycle verbs, per-server action errors, and modal booleans.

import { create } from "zustand";

/** A failed lifecycle dispatch, rendered by the server view (and raised by
 *  palette commands too — structured errors surface wherever they happen). */
export interface ActionError {
  message: string;
  code?: string;
  remediation: string[];
}

interface UiState {
  newServerOpen: boolean;
  paletteOpen: boolean;
  connectionsOpen: boolean;
  /** Verb currently in flight per server ("start" | "stop" | …). */
  pending: Record<string, string>;
  /** Last failed dispatch per server. */
  actionErrors: Record<string, ActionError>;
  setNewServerOpen: (open: boolean) => void;
  setPaletteOpen: (open: boolean) => void;
  setConnectionsOpen: (open: boolean) => void;
  setPending: (serverId: string, verb: string | null) => void;
  setActionError: (serverId: string, error: ActionError | null) => void;
}

export const useUi = create<UiState>()((set) => ({
  newServerOpen: false,
  paletteOpen: false,
  connectionsOpen: false,
  pending: {},
  actionErrors: {},

  setNewServerOpen: (open) => set({ newServerOpen: open }),
  setPaletteOpen: (open) => set({ paletteOpen: open }),
  setConnectionsOpen: (open) => set({ connectionsOpen: open }),
  setPending: (serverId, verb) =>
    set((state) => {
      const pending = { ...state.pending };
      if (verb === null) delete pending[serverId];
      else pending[serverId] = verb;
      return { pending };
    }),

  setActionError: (serverId, error) =>
    set((state) => {
      const actionErrors = { ...state.actionErrors };
      if (error === null) delete actionErrors[serverId];
      else actionErrors[serverId] = error;
      return { actionErrors };
    }),
}));
