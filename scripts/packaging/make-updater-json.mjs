#!/usr/bin/env node
// The updater manifest (ADR-0024): latest.json is the one file the panel's
// update lane reads, so it is generated here — deterministically, from the
// artifacts the release lane just built and signed — instead of by hand.
//
// Usage:
//   node make-updater-json.mjs \
//     --version 0.1.42 \
//     --notes "Development build 0.1.42 from commit abc1234 (develop)." \
//     --base-url "https://github.com/ZaminMC/ZaminPanel/releases/download/dev" \
//     --out latest.json \
//     --entry windows-x86_64=/path/to/ZaminPanel_0.1.42_x64-setup.exe \
//     --entry linux-x86_64=/path/to/ZaminPanel_0.1.42_amd64.AppImage
//
// Every entry's installer must sit next to a `<name>.sig` produced by the
// Tauri bundler; the manifest inlines the signature. A missing signature
// fails the run — an unsigned installer must never be offered (§79).

import { readFileSync, writeFileSync } from "node:fs";
import { basename, join, dirname } from "node:path";

function parseArgs(argv) {
  const args = { entries: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    if (flag === "--entry") {
      const [platform, path] = (argv[++i] ?? "").split("=", 2);
      if (!platform || !path) throw new Error("--entry needs platform=path");
      args.entries.push({ platform, path });
    } else if (flag === "--version") args.version = argv[++i];
    else if (flag === "--notes") args.notes = argv[++i];
    else if (flag === "--base-url") args.baseUrl = argv[++i];
    else if (flag === "--out") args.out = argv[++i];
    else throw new Error(`unknown flag: ${flag}`);
  }
  for (const required of ["version", "notes", "baseUrl", "out"]) {
    if (!args[required]) throw new Error(`missing --${required}`);
  }
  if (args.entries.length === 0) throw new Error("no --entry platforms given");
  return args;
}

const PLATFORM_ALIASES = new Map([
  ["windows-x86_64", "windows-x86_64"],
  ["linux-x86_64", "linux-x86_64"],
]);

const args = parseArgs(process.argv.slice(2));

const platforms = {};
for (const { platform, path } of args.entries) {
  const key = PLATFORM_ALIASES.get(platform);
  if (!key) throw new Error(`unknown platform: ${platform} (expected windows-x86_64 or linux-x86_64)`);
  const signaturePath = `${path}.sig`;
  let signature;
  try {
    signature = readFileSync(signaturePath, "utf8").trim();
  } catch {
    throw new Error(`missing updater signature for ${basename(path)} — run the build with the signing key (expected ${signaturePath})`);
  }
  if (signature === "") throw new Error(`empty updater signature: ${signaturePath}`);
  platforms[key] = {
    signature,
    url: `${args.baseUrl}/${encodeURIComponent(basename(path))}`,
  };
}

const manifest = {
  version: args.version,
  notes: args.notes,
  pub_date: new Date().toISOString(),
  platforms,
};

const outPath = args.out ?? "latest.json";
writeFileSync(join(dirname(outPath), basename(outPath)), `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`updater manifest: ${outPath}`);
console.log(`  version: ${manifest.version}`);
for (const [key, entry] of Object.entries(platforms)) {
  console.log(`  ${key}: ${entry.url}`);
}
