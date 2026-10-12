// The Arix theme port's store (2026-10-12): the customization schema's
// own laws — normalization clamps every knob into its legal range (a
// corrupted localStorage entry degrades to the nearest legal value,
// never to a broken sheet), applyTheme speaks the CSS variables the
// token layer consumes, and persistence round-trips through the same
// key the inline boot script reads (the no-flash contract: the script's
// values and the store's must be the same shape).

import { beforeEach, describe, expect, it } from "vitest";
import {
  THEME_DEFAULTS,
  THEME_KEY,
  applyTheme,
  normalizeTheme,
  readSavedTheme,
} from "./theme";

describe("normalizeTheme", () => {
  it("fills a partial shape with the Arix defaults", () => {
    const theme = normalizeTheme({ mode: "light" });
    expect(theme.mode).toBe("light");
    expect(theme.glass).toBe(THEME_DEFAULTS.glass);
    expect(theme.primary).toBe(THEME_DEFAULTS.primary);
    expect(theme.radiusBox).toBe(THEME_DEFAULTS.radiusBox);
  });

  it("clamps every numeric knob into its legal range", () => {
    const theme = normalizeTheme({
      glassOpacity: 5,
      glassBlur: 400,
      radiusBox: -3,
      radiusInput: 99,
    });
    expect(theme.glassOpacity).toBe(40);
    expect(theme.glassBlur).toBe(40);
    expect(theme.radiusBox).toBe(0);
    expect(theme.radiusInput).toBe(16);
  });

  it("rejects a malformed primary and falls back to the default", () => {
    expect(normalizeTheme({ primary: "not-a-color" }).primary).toBe(THEME_DEFAULTS.primary);
    expect(normalizeTheme({ primary: "#4A35CF" }).primary).toBe("#4a35cf");
    expect(normalizeTheme({ primary: "#12345" }).primary).toBe(THEME_DEFAULTS.primary);
  });

  it("rejects an unknown background preset", () => {
    expect(normalizeTheme({ background: "spyware" }).background).toBe("aurora");
  });

  it("survives garbage input untouched-shaped", () => {
    const theme = normalizeTheme("nonsense");
    expect(theme).toEqual(THEME_DEFAULTS);
  });
});

describe("applyTheme", () => {
  beforeEach(() => {
    document.documentElement.removeAttribute("style");
    delete document.documentElement.dataset.mode;
    delete document.documentElement.dataset.glass;
    delete document.documentElement.dataset.background;
  });

  it("speaks the CSS variables the token layer consumes", () => {
    applyTheme({
      ...THEME_DEFAULTS,
      mode: "light",
      glass: false,
      primary: "#1a73e8",
      radiusBox: 14,
      background: "mesh",
    });
    const root = document.documentElement;
    expect(root.dataset.mode).toBe("light");
    expect(root.dataset.glass).toBe("off");
    expect(root.dataset.background).toBe("mesh");
    expect(root.style.getPropertyValue("--primary")).toBe("#1a73e8");
    expect(root.style.getPropertyValue("--radius-box")).toBe("14px");
    // Arix's light/dark image law: the light mode blurs the LIGHT
    // variant of the preset, never the dark image.
    expect(root.style.getPropertyValue("--background-image")).toContain("mesh-light.jpg");
    // The boot ground follows the mode — the html canvas paints it.
    expect(root.style.getPropertyValue("--boot-ground")).toBe("#f0f1f5");
  });

  it("paints the dark boot ground by default", () => {
    applyTheme(THEME_DEFAULTS);
    expect(document.documentElement.style.getPropertyValue("--boot-ground")).toBe("#0b0d2a");
  });

  it("names no image for the flat preset", () => {
    applyTheme({ ...THEME_DEFAULTS, background: "none" });
    expect(document.documentElement.style.getPropertyValue("--background-image")).toBe("none");
  });
});

describe("readSavedTheme", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("returns the defaults when nothing is saved", () => {
    expect(readSavedTheme()).toEqual(THEME_DEFAULTS);
  });

  it("round-trips a saved shape through the same key the boot script reads", () => {
    const saved = { ...THEME_DEFAULTS, mode: "light", primary: "#0f9d6e" };
    window.localStorage.setItem(THEME_KEY, JSON.stringify(saved));
    expect(readSavedTheme()).toEqual(saved);
  });

  it("degrades a corrupted entry to the nearest legal theme", () => {
    window.localStorage.setItem(THEME_KEY, "{not json at all");
    expect(readSavedTheme()).toEqual(THEME_DEFAULTS);
    window.localStorage.setItem(THEME_KEY, JSON.stringify({ glassOpacity: "huge" }));
    expect(readSavedTheme().glassOpacity).toBe(THEME_DEFAULTS.glassOpacity);
  });
});
