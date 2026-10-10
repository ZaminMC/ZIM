// The real brand marks' law (softwareMarks.ts): the founder's three
// named softwares ride their projects' OWN official assets, and the
// softwares without a fetched mark keep the monogram — an honest
// stand-in never impersonates a brand's real mark.

import { describe, expect, it } from "vitest";
import { SOFTWARE_MARKS, softwareMark } from "./softwareMarks";

describe("the software marks", () => {
  it("the founder's three named softwares carry their official marks", () => {
    expect(Object.keys(SOFTWARE_MARKS).sort()).toEqual([
      "folia",
      "paper",
      "purpur",
    ]);
  });

  it("every mark's src resolved to a bundled asset url", () => {
    for (const mark of Object.values(SOFTWARE_MARKS)) {
      expect(typeof mark.src).toBe("string");
      expect(mark.src.length).toBeGreaterThan(0);
    }
  });

  it("a mark's name mirrors its glyph's name (one identity, two renderings)", () => {
    expect(SOFTWARE_MARKS.paper?.name).toBe("Paper");
    expect(SOFTWARE_MARKS.folia?.name).toBe("Folia");
    expect(SOFTWARE_MARKS.purpur?.name).toBe("Purpur");
  });

  it("the unmarked softwares fall back to null, never undefined-crash", () => {
    expect(softwareMark("fabric")).toBeNull();
    expect(softwareMark("vanilla")).toBeNull();
    expect(softwareMark("unknown")).toBeNull();
  });
});
