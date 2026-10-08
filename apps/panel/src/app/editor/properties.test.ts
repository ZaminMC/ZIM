// The properties AST: line preservation is the contract — a compose edit
// rewrites only the pairs it touched, and every comment, blank line,
// ordering choice, and unknown key survives the round trip byte for
// byte. The control kinds come from the value's own bytes, never a
// hardcoded key list that drifts.

import { describe, expect, it } from "vitest";
import {
  controlFor,
  humanizeKey,
  parseProperties,
  serializeProperties,
  setPropertyValue,
} from "./properties";

describe("properties AST", () => {
  it("round-trips a file it never touched, byte for byte", () => {
    const raw = [
      "# Minecraft server properties",
      "# Edited by hand, as always",
      "",
      "server-port=25565",
      "motd=A Minecraft Server",
      "   # an indented comment stays put",
      "online-mode=true",
      "",
    ].join("\n");
    expect(serializeProperties(parseProperties(raw))).toBe(raw);
  });

  it("edits one pair and leaves the rest of the file alone", () => {
    const raw = ["# header", "server-port=25565", "online-mode=true"].join("\n");
    const lines = setPropertyValue(parseProperties(raw), "server-port", "25566");
    expect(serializeProperties(lines)).toBe(
      ["# header", "server-port=25566", "online-mode=true"].join("\n"),
    );
  });

  it("speaks the file's own dialect: bare keys, colon pairs, spaces", () => {
    const raw = ["bare-key", "colon: value", "spaced key  spaced value"].join("\n");
    const lines = parseProperties(raw);
    expect(lines.map((line) => line.kind)).toEqual(["pair", "pair", "pair"]);
    expect(lines[0]?.value).toBe("");
    expect(lines[1]?.key).toBe("colon");
    expect(lines[1]?.value).toBe("value");
    expect(lines[2]?.key).toBe("spaced");
    // Java's own rule: the first unescaped space ends the key, the rest
    // (after one separator run) is the value verbatim.
    expect(lines[2]?.value).toBe("key  spaced value");
  });

  it("decodes and re-encodes escaped values without inventing any", () => {
    const raw = ["motd=Hello\\nWorld", "path=C\\:\\\\Users"].join("\n");
    const lines = parseProperties(raw);
    expect(lines[0]?.value).toBe("Hello\nWorld");
    expect(lines[1]?.value).toBe("C:\\Users");
    expect(serializeProperties(lines)).toBe(raw);
  });

  it("a compose edit of one line cannot disturb the others", () => {
    const raw = [
      "view-distance=10",
      "# the editor's own note",
      "max-players=20",
      "level-seed=",
    ].join("\n");
    const lines = setPropertyValue(parseProperties(raw), "max-players", "40");
    const out = serializeProperties(lines);
    expect(out).toContain("view-distance=10");
    expect(out).toContain("# the editor's own note");
    expect(out).toContain("max-players=40");
    expect(out).toContain("level-seed=");
  });

  it("humanizes keys into words and reads control kinds from the bytes", () => {
    expect(humanizeKey("server-port")).toBe("Server Port");
    expect(humanizeKey("auto-save.interval_ticks")).toBe("Auto Save Interval Ticks");
    expect(controlFor("true")).toBe("boolean");
    expect(controlFor("false")).toBe("boolean");
    expect(controlFor("25565")).toBe("number");
    expect(controlFor("-5")).toBe("number");
    expect(controlFor("A Minecraft Server")).toBe("text");
    expect(controlFor("")).toBe("text");
  });
});
