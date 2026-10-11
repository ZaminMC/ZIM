// The operator's remembered creation preferences — panel-local storage,
// never daemon state (the daemon receives each choice explicitly on
// every `server.create`; nothing rides on a hidden default). The one
// preference today: automatic EULA acceptance (ADR-0034's sibling rule —
// explicit user configuration, persisted, never silently invented).

import { create } from "zustand";
import { persist } from "zustand/middleware";

const STORAGE_KEY = "zamin.creation";

interface CreationPrefsState {
  /** Remembered "Automatically accept the Minecraft EULA" checkbox. The
   *  modal seeds itself from here; the choice rides every create request
   *  as explicit `autoEula`, so the daemon never guesses. */
  autoEula: boolean;
  setAutoEula: (value: boolean) => void;
}

export const useCreationPrefs = create<CreationPrefsState>()(
  persist(
    (set) => ({
      autoEula: false,

      setAutoEula: (value) => set({ autoEula: value }),
    }),
    { name: STORAGE_KEY },
  ),
);
