// §26's recall lane (consoleHistory): the per-server command history's
// own laws — collapse, move-to-front, cap eviction, corrupted-payload
// degradation — and the two lanes that keep every JS realm in step: the
// same-realm emitter and the cross-realm storage event.

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import {
  HISTORY_CAP,
  consoleHistoryFor,
  recordCommand,
  useConsoleHistory,
} from "./consoleHistory";

const KEY = "zamin.consoleHistory";

/** The realm's snapshot cache must not leak between tests — the storage
 *  event with a null key is the honest wholesale-clear invalidation a
 *  real cross-realm clear produces. */
function invalidateRealmCache(): void {
  window.dispatchEvent(new StorageEvent("storage", { key: null }));
}

beforeEach(() => {
  localStorage.removeItem(KEY);
  invalidateRealmCache();
});

describe("consoleHistory — recording laws", () => {
  it("records per server, newest first", () => {
    recordCommand("s1", "say hi");
    recordCommand("s1", "list");
    recordCommand("s2", "tps");
    expect(consoleHistoryFor("s1")).toEqual(["list", "say hi"]);
    expect(consoleHistoryFor("s2")).toEqual(["tps"]);
  });

  it("a consecutive duplicate collapses; an older duplicate moves to the front", () => {
    recordCommand("s", "a");
    recordCommand("s", "b");
    recordCommand("s", "b"); // consecutive duplicate: no double entry
    expect(consoleHistoryFor("s")).toEqual(["b", "a"]);
    recordCommand("s", "a"); // older duplicate: re-sent, moves to front
    expect(consoleHistoryFor("s")).toEqual(["a", "b"]);
  });

  it("the cap evicts from the oldest end", () => {
    for (let i = 0; i < HISTORY_CAP + 5; i += 1) {
      recordCommand("s", `cmd ${i}`);
    }
    const history = consoleHistoryFor("s");
    expect(history.length).toBe(HISTORY_CAP);
    expect(history[0]).toBe(`cmd ${HISTORY_CAP + 4}`);
    expect(history[HISTORY_CAP - 1]).toBe("cmd 5");
  });

  it("blank commands are never recorded", () => {
    recordCommand("s", "   ");
    expect(consoleHistoryFor("s")).toEqual([]);
  });

  it("the payload is panel-local JSON under its own key", () => {
    recordCommand("s", "list");
    const persisted: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    expect(persisted).toEqual({ s: ["list"] });
  });

  it("a corrupted payload degrades to no history, never a thrown render", () => {
    localStorage.setItem(KEY, "{not json at all");
    invalidateRealmCache();
    expect(consoleHistoryFor("s")).toEqual([]);
    // And the realm keeps working after the degradation.
    recordCommand("s", "list");
    expect(consoleHistoryFor("s")).toEqual(["list"]);
  });

  it("entries that are not strings are dropped on read", () => {
    localStorage.setItem(KEY, JSON.stringify({ s: ["list", 42, null, "tps"] }));
    invalidateRealmCache();
    expect(consoleHistoryFor("s")).toEqual(["list", "tps"]);
  });
});

describe("consoleHistory — the two sync lanes", () => {
  it("same-realm writes re-render the hook", () => {
    const { result } = renderHook(() => useConsoleHistory("s"));
    expect(result.current).toEqual([]);
    act(() => {
      recordCommand("s", "list");
    });
    expect(result.current).toEqual(["list"]);
  });

  it("cross-realm writes ride the storage event", () => {
    const { result } = renderHook(() => useConsoleHistory("s"));
    expect(result.current).toEqual([]);
    // Another realm's write: the DOM is the only channel between realms.
    localStorage.setItem(KEY, JSON.stringify({ s: ["say hello"] }));
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: KEY }));
    });
    expect(result.current).toEqual(["say hello"]);
  });

  it("an unrelated key's storage event does not invalidate the snapshot", () => {
    recordCommand("s", "list");
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "zamin.unrelated" }));
    });
    expect(consoleHistoryFor("s")).toEqual(["list"]);
  });
});
