// Console text primitives: ANSI per level, keystroke → line buffering.

import { describe, expect, it } from "vitest";
import { formatLogLine, formatMarker, LineBuffer } from "./consoleText";

describe("formatLogLine", () => {
  it("colors warn and error lines, dims thread prefixes", () => {
    const warn = formatLogLine({ tsMs: 0, level: "warn", thread: "Server thread", line: "Can't keep up!" });
    expect(warn).toContain("\x1b[33m");
    expect(warn).toContain("\x1b[2m[Server thread]\x1b[0m");

    const error = formatLogLine({ tsMs: 0, level: "error", line: "boom" });
    expect(error).toContain("\x1b[31mboom\x1b[0m");

    const info = formatLogLine({ tsMs: 0, level: "info", line: "Done (3.2s)!" });
    expect(info).toBe("Done (3.2s)!");
  });
});

describe("formatMarker", () => {
  it("wraps text in dim markers", () => {
    expect(formatMarker("reconnected")).toBe("\x1b[2m— reconnected —\x1b[0m");
  });
});

describe("LineBuffer", () => {
  it("completes lines on carriage returns and handles DEL", () => {
    const buffer = new LineBuffer();
    expect(buffer.push("he")).toEqual([]);
    expect(buffer.push("llo")).toEqual([]);
    expect(buffer.push("\r")).toEqual(["hello"]);
    expect(buffer.pending).toBe("");
  });

  it("erases with backspace before the line completes", () => {
    const buffer = new LineBuffer();
    buffer.push("stpo");
    buffer.push("\u007f\u007f");
    buffer.push("op");
    expect(buffer.push("\r")).toEqual(["stop"]);
  });

  it("ignores control characters other than CR and DEL", () => {
    const buffer = new LineBuffer();
    buffer.push("a\u0000b\u001bc");
    expect(buffer.push("\r")).toEqual(["abc"]);
  });

  it("caps absurd line lengths (paste bomb guard)", () => {
    const buffer = new LineBuffer();
    buffer.push("x".repeat(5_000));
    expect(buffer.pending.length).toBe(2_000);
  });
});
