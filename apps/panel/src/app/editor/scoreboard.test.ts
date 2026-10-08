// The scoreboard dialect over the composable row model: reading, and
// the three write paths (title, single, indexed). The invariant under
// test is the slice's promise — an edit rewrites the pairs it names and
// NOTHING else: comments, blanks, ordering, and foreign keys keep their
// bytes; values re-encode in the canonical properties form the compose
// pipeline already writes.

import { describe, expect, it } from "vitest";
import { parseProperties, serializeProperties } from "./properties";
import {
  readScoreboard,
  writeScoreboardTitle,
  writeScoreboardRows,
  scoreboardLayout,
  templateScoreboard,
} from "./scoreboard";

describe("scoreboardLayout", () => {
  it("detects the indexed layout", () => {
    const lines = parseProperties("title=Survival\nline.1=Players: 12\nline.2=Money: $100\n");
    expect(scoreboardLayout(lines)).toBe("indexed");
  });

  it("detects the single-pair layout", () => {
    const lines = parseProperties("title=Survival\nlines=Players: 12,Money: $100\n");
    expect(scoreboardLayout(lines)).toBe("single");
  });

  it("refuses a file whose rows say nothing about scoreboards", () => {
    const lines = parseProperties("server-port=25565\nmotd=A server\n");
    expect(scoreboardLayout(lines)).toBe("undetectable");
  });

  it("accepts hyphen and word separators in row keys", () => {
    expect(scoreboardLayout(parseProperties("title=T\nline-1=a\n"))).toBe("indexed");
    expect(scoreboardLayout(parseProperties("title=T\nrow_2=a\n"))).toBe("indexed");
  });
});

describe("readScoreboard", () => {
  it("reads title and indexed rows in numeric order, keys pinned", () => {
    const spec = readScoreboard(
      parseProperties("title=&c&lSurvival\nline.2=Money: $100\nline.1=Players: 12\nline.10=Tail\n"),
    );
    expect(spec?.layout).toBe("indexed");
    expect(spec?.title).toBe("&c&lSurvival");
    // Numeric order, not lexicographic: 10 comes last.
    expect(spec?.rows.map((row) => row.value)).toEqual(["Players: 12", "Money: $100", "Tail"]);
    expect(spec?.rows.map((row) => row.key)).toEqual(["line.1", "line.2", "line.10"]);
  });

  it("reads the single-pair layout, commas split", () => {
    const spec = readScoreboard(
      parseProperties("title=Survival\nlines=Players: 12,Money: $100,\n"),
    );
    expect(spec?.layout).toBe("single");
    expect(spec?.rows.map((row) => row.value)).toEqual(["Players: 12", "Money: $100", ""]);
  });

  it("returns null for a plain properties file", () => {
    expect(readScoreboard(parseProperties("server-port=25565\n"))).toBeNull();
  });
});

describe("writeScoreboardTitle", () => {
  it("rewrites only the title pair", () => {
    const before = parseProperties(
      "# the main scoreboard\ntitle=Old\nline.1=Players: 12\n\n# keep me\n",
    );
    const after = writeScoreboardTitle(before, "&aNew");
    expect(serializeProperties(after)).toBe(
      "# the main scoreboard\ntitle=&aNew\nline.1=Players\\: 12\n\n# keep me\n",
    );
  });
});

describe("writeScoreboardRows (indexed)", () => {
  it("edits rows in place through their keys", () => {
    const before = parseProperties("title=T\nline.1=Players: 12\nline.2=Money: $100\n");
    const spec = readScoreboard(before)!;
    const after = writeScoreboardRows(before, spec, [
      { key: "line.1", value: "Players: 13" },
      { key: "line.2", value: "Money: $200" },
    ]);
    expect(serializeProperties(after)).toBe(
      "title=T\nline.1=Players\\: 13\nline.2=Money\\: $200\n",
    );
  });

  it("drops the pair of a removed row — a deliberate structural edit", () => {
    const before = parseProperties("title=T\nline.1=A\nline.2=B\nline.3=C\n");
    const spec = readScoreboard(before)!;
    const after = writeScoreboardRows(
      before,
      spec,
      spec.rows.filter((row) => row.value !== "B"),
    );
    expect(serializeProperties(after)).toBe("title=T\nline.1=A\nline.3=C\n");
  });

  it("appends gained rows with the file's own key style, no collisions", () => {
    const before = parseProperties("title=T\nline-1=A\n");
    const spec = readScoreboard(before)!;
    const after = writeScoreboardRows(before, spec, [
      ...spec.rows,
      { key: null, value: "B" },
      { key: null, value: "C" },
    ]);
    expect(serializeProperties(after)).toBe("title=T\nline-1=A\nline-2=B\nline-3=C\n");
  });

  it("never reuses the index of a dropped key when a higher one exists", () => {
    const before = parseProperties("title=T\nline.1=A\nline.5=E\n");
    const spec = readScoreboard(before)!;
    const after = writeScoreboardRows(before, spec, [
      ...spec.rows,
      { key: null, value: "F" },
    ]);
    expect(serializeProperties(after)).toBe("title=T\nline.1=A\nline.5=E\nline.6=F\n");
  });
});

describe("writeScoreboardRows (single)", () => {
  it("rewrites the one pair, joining with commas", () => {
    const before = parseProperties("# hud\ntitle=T\nlines=A,B\n");
    const spec = readScoreboard(before)!;
    const after = writeScoreboardRows(before, spec, [
      { key: "lines", value: "A" },
      { key: "lines", value: "X" },
      { key: "lines", value: "Y" },
    ]);
    expect(serializeProperties(after)).toBe("# hud\ntitle=T\nlines=A,X,Y\n");
  });
});

describe("templateScoreboard", () => {
  it("round-trips through the reader", () => {
    const spec = readScoreboard(parseProperties(templateScoreboard()));
    expect(spec?.title).toBe("&c&lSurvival");
    expect(spec?.rows).toHaveLength(2);
  });
});
