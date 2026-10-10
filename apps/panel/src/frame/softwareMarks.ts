// The software favicon's REAL brand marks — the stand-in monograms'
// replacements (faviconLaw.ts keeps the monograms for the software the
// founder's list didn't name: fabric, vanilla, unknown).
//
// THE PROVENANCE LAW: every mark below is the project's own official
// asset, fetched unmodified (or cropped, never redrawn) —
//
//   paper  assets.papermc.io/brand/papermc_logo.min.svg  (PaperMC's own
//          brand asset; the diamond + paper plane, © PaperMC, MIT-licensed
//          project branding served from their asset host)
//   folia  github.com/PaperMC/Folia → folia.png (the README's logo; the
//          rainbow leaf cropped square at 64px — the crop is the only
//          edit, no colors or shapes redrawn)
//   purpur purpurmc.org/favicon.ico (the site's OWN icon — the same
//          cube grid as their print SVG, but stroked at icon scale; the
//          print SVG's 1.8-unit lines die at 16px, the favicon form is
//          the mark the browser world actually shows)
//
// Each rides as a bundler asset URL (Vite inlines/hoists per size) — no
// runtime fetch, no base64 blob in source, the renderer only ever sees
// a string. A mark is an identifier of third-party software: ZIM shows
// WHICH server software a tab runs; it does not claim those marks.

import paperUrl from "./assets/papermc-logo.svg";
import foliaUrl from "./assets/folia-leaf.png";
import purpurUrl from "./assets/purpur.png";
import type { SoftwareKey } from "./faviconLaw";

export interface SoftwareMark {
  /** The bundled asset URL of the official mark. */
  src: string;
  /** The full name for tooltips and alt text (mirrors the glyph's). */
  name: string;
}

/** The marks the founder named, by software key. The keys WITHOUT a
 *  real mark (fabric, vanilla, unknown) stay absent — the render falls
 *  back to their monogram, so an honest stand-in never impersonates a
 *  brand's real mark. */
export const SOFTWARE_MARKS: Partial<Record<SoftwareKey, SoftwareMark>> = {
  paper: { src: paperUrl, name: "Paper" },
  folia: { src: foliaUrl, name: "Folia" },
  purpur: { src: purpurUrl, name: "Purpur" },
};

/** The mark for a software key, or null when the monogram rides. */
export function softwareMark(key: SoftwareKey): SoftwareMark | null {
  return SOFTWARE_MARKS[key] ?? null;
}
