// The Minecraft formatting model: legacy codes parse into the exact
// segments the client would render. The specialized editors (§34) all
// preview through this — it is the dialect those configs speak.

import { describe, expect, it } from "vitest";
import { parseFormatting, stripFormatting, MC_COLORS } from "./minecraft";

describe("parseFormatting", () => {
  it("splits color and format codes into styled segments", () => {
    const segments = parseFormatting("&c&lSurvival");
    expect(segments).toEqual([
      { text: "Survival", color: "#ff5555", bold: true, italic: false, underline: false, strike: false, obfuscated: false },
    ]);
  });

  it("keeps unstyled text in its own segment", () => {
    const segments = parseFormatting("Players: 12");
    expect(segments).toEqual([
      { text: "Players: 12", color: null, bold: false, italic: false, underline: false, strike: false, obfuscated: false },
    ]);
  });

  it("chains codes across segments", () => {
    const segments = parseFormatting("&aMoney:&f $100");
    expect(segments).toHaveLength(2);
    expect(segments[0]).toMatchObject({ text: "Money:", color: "#55ff55" });
    expect(segments[1]).toMatchObject({ text: " $100", color: "#ffffff" });
  });

  it("resets with &r", () => {
    const segments = parseFormatting("&cRed&r back");
    expect(segments[0]).toMatchObject({ text: "Red", color: "#ff5555" });
    expect(segments[1]).toMatchObject({ text: " back", color: null, bold: false });
  });

  it("accepts the section-sign form the client itself writes", () => {
    const segments = parseFormatting("\u00a7eGold");
    expect(segments[0]).toMatchObject({ text: "Gold", color: "#ffff55" });
  });

  it("keeps an unknown code literally — the preview never eats text", () => {
    const segments = parseFormatting("&xHello");
    expect(segments[0]).toMatchObject({ text: "&xHello" });
  });

  it("keeps a trailing ampersand literally", () => {
    expect(parseFormatting("100% &")[0]).toMatchObject({ text: "100% &" });
  });

  it("covers all sixteen legacy colors", () => {
    for (const code of "0123456789abcdef") {
      expect(MC_COLORS[code]).toMatch(/^#[0-9a-f]{6}$/);
    }
  });

  it("formats underline and strikethrough together", () => {
    const segments = parseFormatting("&n&mBoth");
    expect(segments[0]).toMatchObject({ underline: true, strike: true });
  });
});

describe("stripFormatting", () => {
  it("removes every code for plain lengths", () => {
    expect(stripFormatting("&c&lSurvival")).toBe("Survival");
    expect(stripFormatting("\u00a7aMoney:\u00a7f $100")).toBe("Money: $100");
  });

  it("leaves ordinary text alone", () => {
    expect(stripFormatting("Minecraft & friends")).toBe("Minecraft & friends");
  });
});
