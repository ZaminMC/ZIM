// View state (ADR-0003): open tabs, ordering, active tab are PANEL-LOCAL
// storage, never daemon state. Persisted to localStorage.

import { create } from "zustand";
import { persist } from "zustand/middleware";

/** A failed lifecycle dispatch, rendered by the server view (and raised by
 *  palette commands too — structured errors surface wherever they happen). */
export interface ActionError {
  message: string;
  code?: string;
  remediation: string[];
}

interface UiState {
  openTabs: string[];
  activeTab: string | null;
  newServerOpen: boolean;
  paletteOpen: boolean;
  connectionsOpen: boolean;
  /** Verb currently in flight per server ("start" | "stop" | …). */
  pending: Record<string, string>;
  /** Last failed dispatch per server. */
  actionErrors: Record<string, ActionError>;
  openServer: (serverId: string) => void;
  closeTab: (serverId: string) => void;
  setActive: (serverId: string | null) => void;
  setNewServerOpen: (open: boolean) => void;
  setPaletteOpen: (open: boolean) => void;
  setConnectionsOpen: (open: boolean) => void;
  setPending: (serverId: string, verb: string | null) => void;
  setActionError: (serverId: string, error: ActionError | null) => void;
}

export const useUi = create<UiState>()(
  persist(
    (set) => ({
      openTabs: [],
      activeTab: null,
      newServerOpen: false,
      paletteOpen: false,
      connectionsOpen: false,
      pending: {},
      actionErrors: {},

      openServer: (serverId) =>
        set((state) => ({
          openTabs: state.openTabs.includes(serverId)
            ? state.openTabs
            : [...state.openTabs, serverId],
          activeTab: serverId,
        })),

      closeTab: (serverId) =>
        set((state) => {
          const openTabs = state.openTabs.filter((id) => id !== serverId);
          const activeTab =
            state.activeTab === serverId ? (openTabs.at(-1) ?? null) : state.activeTab;
          return { openTabs, activeTab };
        }),

      setActive: (serverId) => set({ activeTab: serverId }),
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
    }),
    {
      name: "zamin-panel.view",
      partialize: (state) => ({ openTabs: state.openTabs, activeTab: state.activeTab }),
    },
  ),
);
