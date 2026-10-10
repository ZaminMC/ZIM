// The favicon law's pins: the dot's color follows the founder's dialect
// exactly (green online, yellow transitions and console warnings, red
// errors and crashes, blue stopped), a console line's level reduces
// dynamically (a healthy line hands the dot back), and the software
// keys map the daemon's catalog ids onto their glyphs.

import { describe, expect, it } from "vitest";
import {
  FAVICON_DOT_COLORS,
  SOFTWARE_GLYPHS,
  faviconDot,
  reduceConsoleSeverity,
  softwareKey,
} from "./faviconLaw";

describe("the dot's law", () => {
  it("paints blue for every flavor of not-started", () => {
    for (const state of ["not-running", "stopped", "unknown"] as const) {
      expect(faviconDot(state, "none")).toBe("blue");
    }
  });

  it("paints yellow for the transitions", () => {
    for (const state of ["starting", "stopping", "adopting"] as const) {
      expect(faviconDot(state, "none")).toBe("yellow");
    }
  });

  it("paints green for a quiet running server", () => {
    expect(faviconDot("running", "none")).toBe("green");
    expect(faviconDot("running", "ok")).toBe("green");
  });

  it("the console's last word wins while running — warn yellow, error red", () => {
    expect(faviconDot("running", "warn")).toBe("yellow");
    expect(faviconDot("running", "error")).toBe("red");
  });

  it("red for a crash and for a failed preflight, whatever the console said", () => {
    expect(faviconDot("crashed", "ok")).toBe("red");
    expect(faviconDot("failed-preflight", "none")).toBe("red");
  });

  it("every dot has its color", () => {
    for (const dot of ["green", "yellow", "red", "blue"] as const) {
      expect(FAVICON_DOT_COLORS[dot]).toMatch(/^#[0-9a-f]{6}$/);
    }
  });
});

describe("the console severity's reduction", () => {
  it("warn and error set the verdict from nothing", () => {
    expect(reduceConsoleSeverity("none", "warn", "running")).toBe("warn");
    expect(reduceConsoleSeverity("none", "error", "running")).toBe("error");
    expect(reduceConsoleSeverity("none", "error", "not-running")).toBe("error");
  });

  it("a healthy line on a RUNNING server hands the dot back (dynamic)", () => {
    expect(reduceConsoleSeverity("warn", "info", "running")).toBe("ok");
    expect(reduceConsoleSeverity("error", "debug", "running")).toBe("ok");
  });

  it("a healthy line while NOT running changes nothing", () => {
    expect(reduceConsoleSeverity("warn", "info", "not-running")).toBe("warn");
    expect(reduceConsoleSeverity("error", "info", "starting")).toBe("error");
  });

  it("a warn never downgrades a standing error, but an info clears it", () => {
    expect(reduceConsoleSeverity("error", "warn", "running")).toBe("error");
    expect(reduceConsoleSeverity("error", "info", "running")).toBe("ok");
  });
});

describe("the software glyph's key", () => {
  it("maps the catalog's ids and their aliases", () => {
    expect(softwareKey("paper")).toBe("paper");
    expect(softwareKey("PaperMC")).toBe("paper");
    expect(softwareKey("folia")).toBe("folia");
    expect(softwareKey("Purpur")).toBe("purpur");
    expect(softwareKey("fabric-loader")).toBe("fabric");
    expect(softwareKey("vanilla")).toBe("vanilla");
  });

  it("an unknown software keeps the stand-in glyph, never a crash", () => {
    expect(softwareKey("quilt")).toBe("unknown");
    expect(softwareKey(undefined)).toBe("unknown");
    expect(softwareKey("")).toBe("unknown");
  });

  it("every glyph carries its monogram, brand hue, and name", () => {
    expect(SOFTWARE_GLYPHS.paper).toMatchObject({ label: "P", name: "Paper" });
    expect(SOFTWARE_GLYPHS.folia).toMatchObject({ label: "F", name: "Folia" });
    expect(SOFTWARE_GLYPHS.purpur).toMatchObject({
      label: "Pu",
      name: "Purpur",
    });
    expect(SOFTWARE_GLYPHS.fabric).toMatchObject({
      label: "Fa",
      name: "Fabric",
    });
    expect(SOFTWARE_GLYPHS.vanilla).toMatchObject({
      label: "G",
      name: "Vanilla",
    });
    for (const glyph of Object.values(SOFTWARE_GLYPHS)) {
      expect(glyph.bg).toMatch(/^#[0-9a-f]{6}$/);
      expect(glyph.fg).toMatch(/^#[0-9a-f]{6}$/);
    }
  });
});
