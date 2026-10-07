// The window identity's contract (ADR-0018): one minted identity per
// browsing context; handoff births always mint fresh; reloads reuse;
// and when the platform stays silent about navigation, an existing id
// is reused — losing a strip on reload is the one unrecoverable
// mistake this module refuses to risk.

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  __setWindowIdentityForTests,
  currentWindowId,
  resolveWindowIdentity,
} from "./windowIdentity";

afterEach(() => {
  vi.unstubAllGlobals();
  sessionStorage.clear();
  __setWindowIdentityForTests(null);
});

describe("window identity", () => {
  it("a birth mints; with a silent platform an existing id is reused", () => {
    sessionStorage.clear();
    resolveWindowIdentity(null);
    const first = currentWindowId();
    expect(first).toMatch(/^w/);
    expect(sessionStorage.getItem("zamin-panel.window")).toBe(first);
    // Same conditions again (no Navigation Timing in this DOM): the id
    // must not churn — a reload that lost its strip would be fatal.
    resolveWindowIdentity(null);
    expect(currentWindowId()).toBe(first);
  });

  it("a handoff birth always mints fresh, even over a copied id", () => {
    sessionStorage.clear();
    resolveWindowIdentity(null);
    const origin = currentWindowId();
    resolveWindowIdentity("#handoff=h-1");
    const child = currentWindowId();
    expect(child).not.toBe(origin);
    expect(sessionStorage.getItem("zamin-panel.window")).toBe(child);
  });

  it("a declared navigation birth mints fresh over a copied id", () => {
    sessionStorage.setItem("zamin-panel.window", "w-copied");
    vi.stubGlobal("performance", { getEntriesByType: () => [{ type: "navigate" }] });
    resolveWindowIdentity(null);
    expect(currentWindowId()).not.toBe("w-copied");
  });

  it("a declared reload reuses the stored id", () => {
    sessionStorage.setItem("zamin-panel.window", "w-mine");
    vi.stubGlobal("performance", { getEntriesByType: () => [{ type: "reload" }] });
    resolveWindowIdentity(null);
    expect(currentWindowId()).toBe("w-mine");
  });

  it("a denied sessionStorage keeps the boot alive with an in-memory id", () => {
    const spy = vi.spyOn(sessionStorage, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    try {
      resolveWindowIdentity(null);
      expect(currentWindowId()).toMatch(/^w/);
    } finally {
      spy.mockRestore();
    }
  });
});
