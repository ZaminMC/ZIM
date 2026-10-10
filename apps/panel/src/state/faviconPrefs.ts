// The favicon dot's settings switch (the founder's "option in setting to
// disable it"). One localStorage key, written by the settings page and
// read by the frame's render — the frame is its own JS realm, so a
// zustand store would not cross; the storage event keeps live tabs in
// step. Default: on (the dots ARE the feature; disabling is the choice).

import { useEffect, useState } from "react";

const KEY = "zamin.favicon-dots";

export function faviconDotsEnabled(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

export function setFaviconDotsEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(KEY, enabled ? "on" : "off");
  } catch {
    // A blocked storage keeps the dots on for this session — the honest
    // default is the feature, not its absence.
  }
}

/** The reactive read for renderers in OTHER realms: the settings page
 *  writes the key there, the storage event lands here, and the strip
 *  re-renders. (Same-realm writes hear nothing — the toggle itself has
 *  no dots to re-render.) */
export function useFaviconDots(): boolean {
  const [enabled, setEnabled] = useState(faviconDotsEnabled);
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === KEY || event.key === null)
        setEnabled(faviconDotsEnabled());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
  return enabled;
}
