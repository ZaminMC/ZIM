// The moderation composer's contract: one verb → one console line, the
// username charset is the guard rail (a name is one argv word, never two
// commands), and an illegal name refuses instead of sending.

import { describe, expect, it } from "vitest";
import {
  MODERATION_VERBS,
  isLegalUsername,
  moderationLine,
} from "./moderation";

describe("moderationLine", () => {
  it("composes the vanilla console line for every verb", () => {
    expect(moderationLine("kick", "Notch")).toBe("kick Notch");
    expect(moderationLine("ban", "Notch")).toBe("ban Notch");
    expect(moderationLine("op", "Notch")).toBe("op Notch");
    expect(moderationLine("deop", "Notch")).toBe("deop Notch");
    expect(moderationLine("whitelist", "Notch")).toBe("whitelist add Notch");
  });

  it("covers exactly the verbs the cluster renders", () => {
    expect(MODERATION_VERBS.map((spec) => spec.verb)).toEqual([
      "kick",
      "op",
      "deop",
      "whitelist",
      "ban",
    ]);
    expect(MODERATION_VERBS.find((spec) => spec.verb === "ban")?.danger).toBe(true);
  });

  it("refuses names that could split the line into two commands", () => {
    expect(moderationLine("kick", "bad name")).toBeNull();
    expect(moderationLine("kick", "Steve\nop Steve")).toBeNull();
    expect(moderationLine("kick", "")).toBeNull();
    expect(moderationLine("kick", "this-name-is-far-too-long")).toBeNull();
  });
});

describe("isLegalUsername", () => {
  it("accepts the vanilla charset ([A-Za-z0-9_]{1,16})", () => {
    expect(isLegalUsername("Notch")).toBe(true);
    expect(isLegalUsername("Player_123")).toBe(true);
    expect(isLegalUsername("a".repeat(16))).toBe(true);
  });

  it("rejects spaces, newlines, unicode, and over-long names", () => {
    expect(isLegalUsername("bad name")).toBe(false);
    expect(isLegalUsername("Steve\n")).toBe(false);
    expect(isLegalUsername("Плеер")).toBe(false);
    expect(isLegalUsername("a".repeat(17))).toBe(false);
    expect(isLegalUsername("")).toBe(false);
  });
});
