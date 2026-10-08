#!/usr/bin/env node
// The release page's changelog: the commit list since the last published
// build, grouped by conventional type. Bash case patterns with parens
// broke on real subjects ("fix(shell): …"), so this lives in node where
// the grouping is code, not glob punctuation. Inputs ride the env:
//   CHANGELOG — newline-joined commit subjects (from the version job)
//   AUTO      — "true" only on a push-triggered release
import { writeFileSync } from "node:fs";

const changelog = (process.env.CHANGELOG ?? "").trim();
const auto = process.env.AUTO === "true";

if (!auto || changelog === "") {
  writeFileSync(
    "changelog.md",
    "(manual dispatch — static notes)\n",
  );
  process.exit(0);
}

const groups = [
  ["added", /^feat(\([^)]*\))?!?:/],
  ["fixed", /^fix(\([^)]*\))?!?:/],
  ["faster", /^perf(\([^)]*\))?!?:/],
];
const sorted = changelog.split("\n").filter((line) => line.trim() !== "");
const lines = [];
for (const [verb, pattern] of groups) {
  for (const line of sorted) {
    if (pattern.test(line)) {
      const body = line.replace(pattern, "").trim();
      lines.push(`- **${verb}** ${body}`);
    }
  }
}
for (const line of sorted) {
  if (!groups.some(([, pattern]) => pattern.test(line))) {
    lines.push(`- ${line}`);
  }
}
lines.push(
  "",
  "*Channel: development pre-release (ADR-0029). Installed builds update themselves through the built-in updater.*",
);
writeFileSync("changelog.md", `## What's in this build\n\n${lines.join("\n")}\n`);
console.log("changelog.md written:", lines.length, "entries");
