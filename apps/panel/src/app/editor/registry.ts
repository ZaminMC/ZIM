// The specialized editor registry (§34): "do not hardcode every plugin —
// create an extensible editor API." A specialized editor declares a
// detector (path + content sniff) and the registry routes an open file
// to the strongest bidder. Nothing here knows the editors by name at
// import time beyond the registration call, so the future chat/tablist/
// server-list editors are an entry away, and the generic Compose view
// stays the fallback for every file the registry cannot speak for.
//
// Detection is honest: a specialized editor only claims a file its
// detector can also parse back. When two claim it, the higher strength
// wins (exact filename > content shape).

import { parseProperties } from "./properties";
import { scoreboardLayout } from "./scoreboard";

export interface SpecializedEditor {
  id: string;
  /** The mode tab's label (e.g. "Scoreboard"). */
  label: string;
  /** 0..1 — how strongly this editor claims the file. Exact filename
   *  matches claim 1; content-shape claims stay below. */
  strength: (path: string, text: string) => number;
}

const SCOREBOARD_PATH = /(^|\/)scoreboard[^/]*\.properties$/i;

/** The registered editors, strongest claim wins at query time. */
const REGISTRY: SpecializedEditor[] = [
  {
    id: "scoreboard",
    label: "Scoreboard",
    strength: (path, text) => {
      if (SCOREBOARD_PATH.test(path)) return 1;
      // Content shape: a properties file whose rows speak a scoreboard
      // (a title pair plus indexed/single line rows). A generic
      // `server.properties` never matches — its rows say nothing about
      // scoreboards.
      if (!path.toLowerCase().endsWith(".properties")) return 0;
      return scoreboardLayout(parseProperties(text)) !== "undetectable" ? 0.6 : 0;
    },
  },
];

/** The editor that claims this file, or null — null means the generic
 *  Compose view is the whole answer (the common case, kept cheap). */
export function specializedEditorFor(path: string | null, text: string): SpecializedEditor | null {
  if (path === null) return null;
  let best: SpecializedEditor | null = null;
  let bestStrength = 0;
  for (const editor of REGISTRY) {
    const strength = editor.strength(path, text);
    if (strength > bestStrength) {
      best = editor;
      bestStrength = strength;
    }
  }
  return best;
}

/** Test seam: replace the registry's contents for a moment. The real
 *  registry refills afterwards, so a test cannot leave the panel
 *  rerouted. */
export function withEditors(editors: SpecializedEditor[], run: () => void): void {
  const saved = REGISTRY.splice(0, REGISTRY.length, ...editors);
  try {
    run();
  } finally {
    REGISTRY.splice(0, REGISTRY.length, ...saved);
  }
}
