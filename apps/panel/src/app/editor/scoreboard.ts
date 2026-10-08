// The scoreboard dialect (§34): a specialized editor's model, built ON
// the composable row model (editor/properties.ts) rather than beside
// it. The file stays a properties file — the scoreboard editor is a
// face over the same lines Compose edits, so an edit through the
// preview rewrites exactly the pairs it names and every other line
// (comments, blanks, ordering, foreign keys) keeps its bytes.
//
// Two honest layouts exist in the wild, and both stay editable:
//   • single  — one pair carries the rows ("lines=a,b,c"), the plugin
//     splits on commas;
//   • indexed — one pair per row ("line.1=…", "line.2=…", any separator
//     between the word and the index).
// Rows carry their pair's KEY: removing a row drops THAT pair (byte-
// stable elsewhere), adding one appends the next index, reordering
// swaps values through the same row model. Placeholders (%online% and
// friends) belong to the plugin: the preview shows them literally
// instead of pretending to resolve them. Values re-encode in the
// canonical properties form when written — the compose pipeline's own
// convention since §33.

import { type PropertyLine } from "./properties";

/** The layout a file's rows speak. `undetectable` means the rows do not
 *  name a scoreboard at all (no title pair, no row keys) — the registry
 *  only routes a file here when this says otherwise. */
export type ScoreboardLayout = "single" | "indexed" | "undetectable";

/** One editable row. `key` is the pair it came from — the removal of a
 *  row drops exactly that pair; a new row arrives with `key: null` and
 *  is assigned the file's next index on write. */
export interface ScoreboardRow {
  key: string | null;
  value: string;
}

export interface ScoreboardSpec {
  layout: Exclude<ScoreboardLayout, "undetectable">;
  /** The title pair's key (the pair a title edit rewrites). */
  titleKey: string;
  /** Raw title value, codes included — the preview parses it. */
  title: string;
  /** Raw row values in order, codes included, each with its pair key. */
  rows: ScoreboardRow[];
}

const ROW_KEY = /^(?:lines?|row)[._-]?(\d+)$/i;
const MAX_ROWS = 32;

function pairOf(lines: PropertyLine[], key: string): PropertyLine | undefined {
  return lines.find((line) => line.kind === "pair" && line.key === key);
}

/** Which layout these rows speak, and where the rows live. Exported for
 *  the registry's content sniff. */
export function scoreboardLayout(lines: PropertyLine[]): ScoreboardLayout {
  if (pairOf(lines, "title") === undefined) return "undetectable";
  if (pairOf(lines, "lines") !== undefined) return "single";
  const indexed = lines.some(
    (line) => line.kind === "pair" && line.key !== undefined && ROW_KEY.test(line.key),
  );
  return indexed ? "indexed" : "undetectable";
}

/** The scoreboard spec these rows currently speak, or null when they
 *  are not a scoreboard configuration. */
export function readScoreboard(lines: PropertyLine[]): ScoreboardSpec | null {
  const layout = scoreboardLayout(lines);
  if (layout === "undetectable") return null;
  const titlePair = pairOf(lines, "title");
  if (!titlePair) return null;

  if (layout === "single") {
    const linesPair = pairOf(lines, "lines");
    const raw = linesPair?.value ?? "";
    return {
      layout,
      titleKey: titlePair.key ?? "title",
      title: titlePair.value ?? "",
      rows: splitSingle(raw).map((value) => ({ key: linesPair?.key ?? "lines", value })),
    };
  }

  const rows: { index: number; key: string; value: string }[] = [];
  for (const line of lines) {
    if (line.kind !== "pair" || line.key === undefined) continue;
    const match = ROW_KEY.exec(line.key);
    if (match?.[1]) rows.push({ index: Number(match[1]), key: line.key, value: line.value ?? "" });
  }
  rows.sort((a, b) => a.index - b.index);
  return {
    layout: "indexed",
    titleKey: titlePair.key ?? "title",
    title: titlePair.value ?? "",
    rows: rows.map((row) => ({ key: row.key, value: row.value })),
  };
}

/** `a,b,c` — commas split; a value may also carry the plugin's own
 *  newline form, which counts as another split point. Empty entries are
 *  kept: a blank row renders as a real (empty) scoreboard row. */
function splitSingle(raw: string): string[] {
  if (raw === "") return [];
  return raw.split(/\n|\\n/).flatMap((part) => part.split(","));
}

/** One pair edit, through the row model — never a reserialization. */
function withPair(lines: PropertyLine[], key: string, value: string): PropertyLine[] {
  return lines.map((line) =>
    line.kind === "pair" && line.key === key ? { ...line, value } : line,
  );
}

export function writeScoreboardTitle(lines: PropertyLine[], title: string): PropertyLine[] {
  return withPair(lines, "title", title);
}

/** Rewrite the rows through the row model.
 *  • single — every row is a view of the one pair; the write joins the
 *    values with commas and rewrites that pair alone.
 *  • indexed — a row keeps its pair (value rewritten), a removed row
 *    drops its pair (a deliberate structural edit), a new row appends
 *    with the file's next index in the file's own key style. */
export function writeScoreboardRows(
  lines: PropertyLine[],
  spec: ScoreboardSpec,
  rows: ScoreboardRow[],
): PropertyLine[] {
  const capped = rows.slice(0, MAX_ROWS);
  if (spec.layout === "single") {
    const key = spec.rows[0]?.key ?? "lines";
    return withPair(lines, key, capped.map((row) => row.value).join(","));
  }

  let next = lines;
  const keptKeys = new Set<string>();
  for (const row of capped) {
    if (row.key === null) continue;
    keptKeys.add(row.key);
    next = withPair(next, row.key, row.value);
  }
  // Removed rows: their pairs leave the file — a deliberate structural
  // edit, exactly the rows the operator deleted.
  for (const current of spec.rows) {
    if (current.key === null || keptKeys.has(current.key)) continue;
    next = next.filter((line) => !(line.kind === "pair" && line.key === current.key));
  }

  // Gained rows: append after the last kept pair, numbered past every
  // index the file currently speaks (no collision, no reuse).
  const gained = capped.filter((row) => row.key === null);
  if (gained.length > 0) {
    let highest = 0;
    let lastKeptKey: string | null = null;
    for (const line of next) {
      if (line.kind !== "pair" || line.key === undefined) continue;
      const match = ROW_KEY.exec(line.key);
      if (match?.[1]) {
        highest = Math.max(highest, Number(match[1]));
        lastKeptKey = line.key;
      }
    }
    const insertAt = next.findIndex(
      (line) => line.kind === "pair" && line.key === lastKeptKey,
    );
    const additions = gained.map((row) => {
      highest += 1;
      const key = nextRowKey(highest, next);
      const line: PropertyLine = { kind: "pair", raw: `${key}=${row.value}`, key, value: row.value };
      return line;
    });
    const at = insertAt === -1 ? next.length : insertAt + 1;
    next = [...next.slice(0, at), ...additions, ...next.slice(at)];
  }
  return next;
}

/** The key for a gained row, in the style the file already speaks
 *  (`line.1` stays `line.4`; `line-1` stays `line-4`). */
function nextRowKey(oneBased: number, lines: PropertyLine[]): string {
  const sample = lines.find(
    (line) => line.kind === "pair" && line.key !== undefined && ROW_KEY.test(line.key),
  );
  const match = sample?.key ? /^([a-zA-Z]+)([._-]?)(\d+)$/.exec(sample.key) : null;
  const word = match?.[1] ?? "line";
  const sep = match?.[2] ?? ".";
  return `${word}${sep}${oneBased}`;
}

/** A fresh, minimal scoreboard file the specialized view can seed (the
 *  editor offers "start from a scoreboard template" when the operator
 *  opens an unrelated properties file and wants the scoreboard shape). */
export function templateScoreboard(title = "&c&lSurvival"): string {
  return [`title=${title}`, "line.1=Players: 12", "line.2=Money: $100", ""].join("\n");
}

export { MAX_ROWS };
