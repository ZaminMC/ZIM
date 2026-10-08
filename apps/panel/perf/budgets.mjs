// Panel bundle budgets (PERFORMANCE-BUDGETS.md, Panel section).
// Reads the production build in dist/ and asserts gzip sizes so a chunk
// that silently regrows fails CI instead of a cold start. Run after
// `npm run build` (the `perf:budgets` npm script does both gates' second
// half; CI wires build + budgets together).

import { gzipSync } from "node:zlib";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const dist = fileURLToPath(new URL("../dist/", import.meta.url));

// Budgets in gzip bytes. The entry holds React, zustand, the shell, the
// dashboard and the workspace chrome; the modals split individually. Since
// ADR-0020 the console is the structured view — no terminal dependency —
// so its chunk shrank to the shared feed engine.
const BUDGETS = {
  // 90 → 96 KB at ADR-0024: the update lane joins the boot chrome (the
  // store + notice + the Settings updates rows ride the entry by design —
  // the boot check is the lane's first duty). The desktop plugin APIs
  // themselves split into lazy chunks, so the growth is the decisions,
  // not the OS calls.
  entryJsGzip: 96 * 1024, // index-*.js — was 147 KB before the split
  anySingleJsGzip: 96 * 1024, // no chunk may quietly become the new monster
  // 170 → 184 KB at ADR-0019: the three configuration surfaces (Startup,
  // Network, Settings) lazy-load into their own chunks (~6.5 KB gzip
  // combined), so the entry and the cold start are untouched; the total
  // grows because the surfaces are real features, not regressions.
  totalJsGzip: 184 * 1024,
  // 12 → 14 KB at ADR-0019 (one shared stylesheet for the config rows);
  // 14 → 15 KB at ADR-0023: the bookmarks bar joins the chrome (its own
  // stylesheet, shared across windows) and the scoreboard editor's two
  // panes ride the files chunk — feature surface, not boot bloat; the
  // entry CSS grew only by the crash card's reason row.
  totalCssGzip: 15 * 1024,
};

function assets() {
  try {
    return readdirSync(join(dist, "assets"));
  } catch {
    console.error("no dist/assets — run `npm run build` first");
    process.exit(2);
  }
}

function gzipSize(file) {
  return gzipSync(readFileSync(join(dist, "assets", file))).length;
}

const files = assets().filter((f) => /\.(js|css)$/.test(f));
if (files.length === 0) {
  console.error("dist/assets holds no js/css — build output missing");
  process.exit(2);
}

const rows = [];
let totalJs = 0;
let totalCss = 0;
let entry = 0;
let worstJs = { file: "-", size: 0 };

for (const file of files) {
  const size = gzipSize(file);
  rows.push([file, size]);
  if (file.endsWith(".js")) {
    totalJs += size;
    if (/^index-.*\.js$/.test(file)) entry += size;
    if (size > worstJs.size) worstJs = { file, size };
  } else {
    totalCss += size;
  }
}

rows.sort((a, b) => b[1] - a[1]);
console.log("gzip sizes (budget report):");
for (const [file, size] of rows) {
  console.log(`  ${size.toString().padStart(8)}  ${file}`);
}
console.log(`  total js:  ${totalJs}`);
console.log(`  total css: ${totalCss}`);

const failures = [];
if (entry > BUDGETS.entryJsGzip) {
  failures.push(`entry chunk ${entry} > ${BUDGETS.entryJsGzip} gzip`);
}
if (worstJs.size > BUDGETS.anySingleJsGzip) {
  failures.push(`chunk ${worstJs.file} (${worstJs.size}) > ${BUDGETS.anySingleJsGzip} gzip`);
}
if (totalJs > BUDGETS.totalJsGzip) {
  failures.push(`total js ${totalJs} > ${BUDGETS.totalJsGzip} gzip`);
}
if (totalCss > BUDGETS.totalCssGzip) {
  failures.push(`total css ${totalCss} > ${BUDGETS.totalCssGzip} gzip`);
}

if (failures.length > 0) {
  console.error("\nBUDGET FAILURES:");
  for (const failure of failures) console.error(`  - ${failure}`);
  console.error("negotiate in PERFORMANCE-BUDGETS.md (rules: fix or write it down)");
  process.exit(1);
}
console.log("\nbundle budgets: PASS");
