// Minecraft legacy formatting codes (§34): the one dialect every
// specialized text preview shares — scoreboard, chat, tablist, server
// list. Plugins write `&c&lSurvival` or `§c§lSurvival`; the preview
// renders the same segments the client would. Pure model, no React —
// the future chat/tablist editors compose the same segments.

export interface McSegment {
  text: string;
  color: string | null;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
  obfuscated: boolean;
}

/** The 16 legacy color codes with the client's own palette (the modern
 *  hex values the vanilla renderer uses — the preview matches what a
 *  player sees, not a loose approximation). */
export const MC_COLORS: Record<string, string> = {
  "0": "#000000",
  "1": "#0000aa",
  "2": "#00aa00",
  "3": "#00aaaa",
  "4": "#aa0000",
  "5": "#aa00aa",
  "6": "#ffaa00",
  "7": "#aaaaaa",
  "8": "#555555",
  "9": "#5555ff",
  a: "#55ff55",
  b: "#55ffff",
  c: "#ff5555",
  d: "#ff55ff",
  e: "#ffff55",
  f: "#ffffff",
};

const FORMATS: Record<string, keyof Omit<McSegment, "text" | "color">> = {
  l: "bold",
  o: "italic",
  n: "underline",
  m: "strike",
  k: "obfuscated",
};

/** Parse `&`/`§` codes into styled segments. An unknown code character
 *  after `&` is kept literally — the preview never silently eats text
 *  the author typed. */
export function parseFormatting(text: string): McSegment[] {
  const segments: McSegment[] = [];
  let current: McSegment = blank();
  let buffer = "";

  const flush = () => {
    if (buffer !== "") {
      segments.push({ ...current, text: buffer });
      buffer = "";
    }
  };

  for (let i = 0; i < text.length; i += 1) {
    const ch = text.charAt(i);
    if ((ch !== "&" && ch !== "\u00a7") || i + 1 >= text.length) {
      buffer += ch;
      continue;
    }
    const code = text.charAt(i + 1).toLowerCase();
    const color = MC_COLORS[code];
    if (color !== undefined) {
      flush();
      current = { ...blank(), color };
      i += 1;
      continue;
    }
    if (code === "r") {
      flush();
      current = blank();
      i += 1;
      continue;
    }
    const format = FORMATS[code];
    if (format) {
      flush();
      current = { ...current, [format]: true };
      i += 1;
      continue;
    }
    // Not a code this dialect knows: keep both characters literally.
    buffer += ch;
  }
  flush();
  return segments;
}

function blank(): McSegment {
  return {
    text: "",
    color: null,
    bold: false,
    italic: false,
    underline: false,
    strike: false,
    obfuscated: false,
  };
}

/** Strip every code — for plain-text lengths and fallbacks. */
export function stripFormatting(text: string): string {
  return text.replace(/[&\u00a7][0-9a-fk-orA-FK-OR]/g, "");
}
