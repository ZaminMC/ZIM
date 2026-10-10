// The tab favicon's law — server identity + liveness in one 16px glyph.
//
// The founder's dialect (the dot):
//   green  — the server is online (running) and the console is quiet
//   yellow — a transition (starting/stopping/adopting), or the console's
//            last relevant line was a WARNING while running — a later
//            healthy line hands the dot back to green (dynamic)
//   red    — the console's last relevant line was an ERROR, or the
//            server stopped by crash (the registry keeps `crashed` until
//            the next start, so the red is as sticky as the truth)
//   blue   — registered, not started
//
// The glyph is the server software's own mark: paper / folia / purpur /
// fabric / vanilla ride monograms in their brand hues until real brand
// SVGs land (the checklist rows the eyeball pass owns). Pure functions
// only — the wire and the React rendering live in favicon.ts / FrameApp.

import type { LogLevel, ServerState } from "../protocol/types";

export type FaviconDot = "green" | "yellow" | "red" | "blue";

/** The console's reduced verdict per server: the last relevant line's
 *  level ("none" = nothing heard since the server went quiet). */
export type ConsoleSeverity = "none" | "warn" | "error" | "ok";

/** One log line, reduced: an ERROR sets red; a WARN never downgrades a
 *  standing error (warnings are the floor, not the cure); an INFO/DEBUG
 *  line from a RUNNING server clears the verdict — the dynamic return
 *  the founder's law asks for (yellow when a warning threw, back to
 *  green while the server is active). */
export function reduceConsoleSeverity(
  current: ConsoleSeverity,
  level: LogLevel,
  state: ServerState,
): ConsoleSeverity {
  if (level === "error") return "error";
  if (state === "running" && (level === "info" || level === "debug"))
    return "ok";
  if (level === "warn") return current === "error" ? "error" : "warn";
  return current;
}

export function faviconDot(
  state: ServerState,
  severity: ConsoleSeverity,
): FaviconDot {
  switch (state) {
    case "crashed":
      return "red";
    case "failed-preflight":
      // The server cannot even preflight — the honest color is red.
      return "red";
    case "starting":
    case "stopping":
    case "adopting":
      return "yellow";
    case "running":
      if (severity === "error") return "red";
      if (severity === "warn") return "yellow";
      return "green";
    case "not-running":
    case "stopped":
    case "unknown":
      return "blue";
  }
}

// --- the software glyph law -------------------------------------------------

export type SoftwareKey =
  "paper" | "folia" | "purpur" | "fabric" | "vanilla" | "unknown";

export interface SoftwareGlyph {
  /** The monogram letters (one or two — 16px-readable). */
  label: string;
  /** The glyph's background — the software's brand hue (the honest
   *  stand-in for the real mark until brand SVGs land). */
  bg: string;
  fg: string;
  /** The full name for tooltips and alt text. */
  name: string;
}

export const SOFTWARE_GLYPHS: Record<SoftwareKey, SoftwareGlyph> = {
  paper: { label: "P", bg: "#0179a3", fg: "#ffffff", name: "Paper" },
  folia: { label: "F", bg: "#67a036", fg: "#ffffff", name: "Folia" },
  purpur: { label: "Pu", bg: "#82409c", fg: "#ffffff", name: "Purpur" },
  fabric: { label: "Fa", bg: "#b8895a", fg: "#ffffff", name: "Fabric" },
  vanilla: { label: "G", bg: "#5b8731", fg: "#ffffff", name: "Vanilla" },
  unknown: { label: "Z", bg: "#5f6368", fg: "#ffffff", name: "Server" },
};

/** The daemon's software string → the glyph key (case-insensitive; the
 *  catalog ids are lowercase, the registry echoes them). */
export function softwareKey(software?: string): SoftwareKey {
  const key = (software ?? "").trim().toLowerCase();
  if (key === "paper" || key === "papermc") return "paper";
  if (key === "folia") return "folia";
  if (key === "purpur") return "purpur";
  if (key === "fabric" || key === "fabric-loader") return "fabric";
  if (key === "vanilla") return "vanilla";
  return "unknown";
}

// --- the dot's colors (the palette the strip renders) -----------------------

export const FAVICON_DOT_COLORS: Record<FaviconDot, string> = {
  green: "#31a349",
  yellow: "#f9ab00",
  red: "#d93025",
  blue: "#1a73e8",
};
