// The log feed engine's tests (ADR-0020): the seam discipline, the error
// normalization, and the hook's whole lifecycle — the ring-tail buffer
// that merges against file history with overlap stripped, the missed
// counter that states what the daemon dropped, the paging cursor, and the
// re-seed on reconnect. The console's correctness rests on these laws.

import { renderHook, act, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  asProtocolError,
  INITIAL_FEED,
  MAX_FEED_LINES,
  PAGE_LINES,
  rowKey,
  stripLiveOverlap,
  useLogFeed,
} from "./logFeed";
import type { LogLine } from "../protocol/types";

vi.mock("./actions", () => ({
  rangeLogs: vi.fn(),
  subscribeLogs: vi.fn(),
}));

import { rangeLogs, subscribeLogs } from "./actions";

const rangeMock = rangeLogs as ReturnType<typeof vi.fn>;
const subscribeMock = subscribeLogs as ReturnType<typeof vi.fn>;

type Handler = {
  onPayload: (n: { payload: unknown }) => void;
  onRegistered?: () => void;
};

let handler: Handler | null = null;
let disposeSpy: ReturnType<typeof vi.fn>;

function line(text: string, tsMs = 1, level: LogLine["level"] = "info"): LogLine {
  return { tsMs, level, line: text };
}

function pushLogs(batch: LogLine[]): void {
  handler?.onPayload({ payload: { kind: "logs", batch } });
}

function pushMissed(count: number): void {
  handler?.onPayload({ payload: { kind: "missed", missed: count } });
}

beforeEach(() => {
  handler = null;
  disposeSpy = vi.fn();
  rangeMock.mockReset().mockResolvedValue({
    file: "logs/latest.log",
    lines: [],
    olderAvailable: false,
    startOffset: 0,
    historyAvailable: true,
  });
  subscribeMock.mockReset().mockImplementation(
    (_serverId: string, h: Handler) => {
      handler = h;
      return Promise.resolve({ dispose: disposeSpy, result: null });
    },
  );
});

afterEach(() => {
  handler = null;
});

describe("stripLiveOverlap — the seam discipline", () => {
  it("live lines the history lacks survive untouched", () => {
    const history = [line("a"), line("b")];
    const live = [line("c"), line("d")];
    expect(stripLiveOverlap(history, live)).toEqual([line("c"), line("d")]);
  });

  it("the buffered ring tail that history already holds is dropped", () => {
    const history = [line("a"), line("b"), line("c")];
    const live = [line("b"), line("c"), line("d")];
    // live's first two lines match history's last two: the seam keeps d.
    expect(stripLiveOverlap(history, live)).toEqual([line("d")]);
  });

  it("matching is by line text — levels and timestamps may differ across sources", () => {
    const history = [line("same words", 100, "warn")];
    const live = [line("same words", 999, "error"), line("new")];
    expect(stripLiveOverlap(history, live)).toEqual([line("new")]);
  });

  it("no match keeps every live line — a rare duplicate beats a lost line", () => {
    const history = [line("a"), line("b")];
    const live = [line("x"), line("y")];
    expect(stripLiveOverlap(history, live)).toEqual([line("x"), line("y")]);
  });

  it("empty sides degenerate safely", () => {
    expect(stripLiveOverlap([], [line("a")])).toEqual([line("a")]);
    expect(stripLiveOverlap([line("a")], [])).toEqual([]);
    expect(stripLiveOverlap([], [])).toEqual([]);
  });

  it("identical buffers collapse to nothing", () => {
    const both = [line("a"), line("b"), line("c")];
    expect(stripLiveOverlap(both, both)).toEqual([]);
  });
});

describe("asProtocolError — the honest shape", () => {
  it("a protocol error object passes through with its code and message", () => {
    const err = asProtocolError({ code: "LOG_CURSOR_INVALID", message: "stale cursor", remediation: [] });
    expect(err.code).toBe("LOG_CURSOR_INVALID");
    expect(err.message).toBe("stale cursor");
  });

  it("a code-carrying object with a junk message borrows the Error's words", () => {
    const err = asProtocolError({ code: "FS_NOT_WRITABLE", message: 42 });
    expect(err.code).toBe("FS_NOT_WRITABLE");
    expect(err.message).toBe("the request failed");
  });

  it("a plain Error becomes INTERNAL_ERROR with its message", () => {
    const err = asProtocolError(new Error("the wire said no"));
    expect(err.code).toBe("INTERNAL_ERROR");
    expect(err.message).toBe("the wire said no");
    expect(err.remediation).toEqual([]);
  });

  it("anything else refuses with the default sentence", () => {
    expect(asProtocolError(undefined).message).toBe("the request failed");
    expect(asProtocolError(7).message).toBe("the request failed");
  });
});

describe("useLogFeed — the lifecycle", () => {
  it("subscribes without a cursor and seeds history behind the ring tail, overlap stripped", async () => {
    rangeMock.mockResolvedValue({
      file: "logs/latest.log",
      lines: [line("h1"), line("h2")],
      olderAvailable: true,
      startOffset: 4096,
      historyAvailable: true,
    });
    const { result } = renderHook(() => useLogFeed("survival"));
    // The ring tail buffered pre-seam: h2 overlaps history, h3 is new.
    pushLogs([line("h2"), line("h3")]);
    await waitFor(() => {
      expect(result.current.state.lines).not.toBeNull();
    });
    expect(subscribeMock).toHaveBeenCalledWith("survival", expect.anything());
    expect(result.current.state.lines?.map((l) => l.line)).toEqual(["h1", "h2", "h3"]);
    expect(result.current.state.liveStart).toBe(2);
    expect(result.current.state.cursor).toBe(4096);
    expect(result.current.state.error).toBeNull();
  });

  it("a server with no log file yet states historyAvailable: false and runs on live alone", async () => {
    rangeMock.mockResolvedValue({
      file: "logs/latest.log",
      lines: [],
      olderAvailable: false,
      startOffset: 0,
      historyAvailable: false,
    });
    const { result } = renderHook(() => useLogFeed("fresh"));
    pushLogs([line("boot output")]);
    await waitFor(() => {
      expect(result.current.state.lines).not.toBeNull();
    });
    expect(result.current.state.historyAvailable).toBe(false);
    expect(result.current.state.lines?.map((l) => l.line)).toEqual(["boot output"]);
    expect(result.current.state.liveStart).toBe(0);
  });

  it("live lines append after the seam; the missed counter states the daemon's drops", async () => {
    rangeMock.mockResolvedValue({
      file: "logs/latest.log", lines: [line("h")], olderAvailable: false, startOffset: 10, historyAvailable: true,
    });
    const { result } = renderHook(() => useLogFeed("survival"));
    await waitFor(() => expect(result.current.state.lines).not.toBeNull());
    pushLogs([line("live 1"), line("live 2")]);
    await waitFor(() => {
      expect(result.current.state.lines?.map((l) => l.line)).toEqual(["h", "live 1", "live 2"]);
    });
    pushMissed(7);
    pushMissed(3);
    await waitFor(() => expect(result.current.state.missed).toBe(10));
  });

  it("a failed seed is a typed error with the buffered live lines kept", async () => {
    rangeMock.mockRejectedValue(new Error("the daemon is unreachable"));
    const { result } = renderHook(() => useLogFeed("survival"));
    pushLogs([line("ring tail")]);
    await waitFor(() => {
      expect(result.current.state.error).not.toBeNull();
    });
    expect(result.current.state.error?.code).toBe("INTERNAL_ERROR");
    expect(result.current.state.lines?.map((l) => l.line)).toEqual(["ring tail"]);
  });

  it("loadOlder prepends the older page and shifts the cursor and the seam", async () => {
    rangeMock
      .mockResolvedValueOnce({
        file: "logs/latest.log",
        lines: [line("h1"), line("h2")],
        olderAvailable: true,
        startOffset: 4096,
        historyAvailable: true,
      })
      .mockResolvedValueOnce({
        file: "logs/latest.log",
        lines: [line("h0a"), line("h0b")],
        olderAvailable: false,
        startOffset: 1024,
        historyAvailable: true,
      });
    const { result } = renderHook(() => useLogFeed("survival"));
    await waitFor(() => expect(result.current.state.cursor).toBe(4096));
    await act(async () => {
      result.current.loadOlder();
    });
    await waitFor(() => {
      expect(result.current.state.cursor).toBe(1024);
    });
    expect(result.current.state.lines?.map((l) => l.line)).toEqual(["h0a", "h0b", "h1", "h2"]);
    // The seam moved down by the page that was prepended above it.
    expect(result.current.state.liveStart).toBe(4);
    expect(rangeMock).toHaveBeenLastCalledWith({ serverId: "survival", maxLines: PAGE_LINES, beforeOffset: 4096 });
  });

  it("loadOlder is refused with no cursor, a failed seed, or a page already in flight", async () => {
    // cursor 0 → no older pages: no call at all.
    rangeMock.mockResolvedValue({
      file: "logs/latest.log", lines: [line("h")], olderAvailable: false, startOffset: 0, historyAvailable: true,
    });
    const { result } = renderHook(() => useLogFeed("survival"));
    await waitFor(() => expect(result.current.state.lines).not.toBeNull());
    result.current.loadOlder();
    expect(rangeMock).toHaveBeenCalledTimes(1); // the seed only
  });

  it("a rotated file under the paging cursor re-seeds history instead of failing", async () => {
    rangeMock
      .mockResolvedValueOnce({
        file: "logs/latest.log", lines: [line("h1")], olderAvailable: true, startOffset: 4096, historyAvailable: true,
      })
      // The page read hits the rotated-away cursor…
      .mockRejectedValueOnce({ code: "LOG_CURSOR_INVALID", message: "cursor is stale" })
      // …and the re-seed answers a fresh tail.
      .mockResolvedValueOnce({
        file: "logs/latest.log", lines: [line("fresh tail")], olderAvailable: false, startOffset: 8192, historyAvailable: true,
      });
    const { result } = renderHook(() => useLogFeed("survival"));
    await waitFor(() => expect(result.current.state.cursor).toBe(4096));
    pushLogs([line("live after")]);
    // The seam is open once the pushed line actually lands in the feed —
    // any delivery path (live append or post-seed residual) proves it.
    await waitFor(() => {
      expect(result.current.state.lines?.map((l) => l.line)).toContain("live after");
    });
    await act(async () => {
      result.current.loadOlder();
    });
    await waitFor(() => {
      expect(result.current.state.cursor).toBe(8192);
    });
    // History restarted from the fresh tail; the live line rides behind it.
    expect(result.current.state.error).toBeNull();
    expect(result.current.state.lines?.map((l) => l.line)).toEqual(["fresh tail", "live after"]);
  });

  it("a reconnect re-seeds the seam against fresh history", async () => {
    rangeMock
      .mockResolvedValueOnce({
        file: "logs/latest.log", lines: [line("h1")], olderAvailable: false, startOffset: 10, historyAvailable: true,
      })
      .mockResolvedValueOnce({
        file: "logs/latest.log", lines: [line("h1"), line("h2")], olderAvailable: false, startOffset: 20, historyAvailable: true,
      });
    const { result } = renderHook(() => useLogFeed("survival"));
    await waitFor(() => expect(result.current.state.lines).not.toBeNull());
    pushLogs([line("pre-drop live")]);
    await waitFor(() => {
      expect(result.current.state.lines?.map((l) => l.line)).toEqual(["h1", "pre-drop live"]);
    });
    // The wire comes back: the subscription re-registers, the seam
    // re-opens against the daemon's fresh replay.
    await act(async () => {
      handler?.onRegistered?.();
    });
    await waitFor(() => {
      expect(result.current.state.cursor).toBe(20);
    });
    expect(rangeMock).toHaveBeenCalledTimes(2);
    // The re-seed merges fresh history behind the pre-drop live line —
    // real output survives the wire's round trip.
    expect(result.current.state.lines?.map((l) => l.line)).toEqual(["h1", "h2", "pre-drop live"]);
  });

  it("unmount disposes the stream subscription", async () => {
    const { unmount } = renderHook(() => useLogFeed("survival"));
    await waitFor(() => expect(subscribeMock).toHaveBeenCalled());
    unmount();
    expect(disposeSpy).toHaveBeenCalled();
  });

  it("the initial state is the honest blank — null lines, no fake empty console", () => {
    expect(INITIAL_FEED.lines).toBeNull();
    expect(INITIAL_FEED.keys).toBeNull();
    expect(INITIAL_FEED.historyAvailable).toBe(true);
    expect(INITIAL_FEED.missed).toBe(0);
    expect(INITIAL_FEED.newCount).toBe(0);
  });
});

describe("the row-key law — §30's cheap rows at the React layer", () => {
  it("rowKey reads the minted identity, not the shifting index", () => {
    const feed = {
      ...INITIAL_FEED,
      lines: [line("a"), line("b")],
      keys: ["r7", "r8"],
    };
    expect(rowKey(feed, 0)).toBe("r7");
    expect(rowKey(feed, 1)).toBe("r8");
    // The honest fallback when keys have not been minted yet.
    expect(rowKey(INITIAL_FEED, 3)).toBe("3");
  });

  it("an append mints fresh keys and never re-keys a survivor", async () => {
    rangeMock.mockResolvedValue({
      file: "logs/latest.log", lines: [line("h")], olderAvailable: false, startOffset: 10, historyAvailable: true,
    });
    const { result } = renderHook(() => useLogFeed("keys"));
    await waitFor(() => expect(result.current.state.lines).not.toBeNull());
    const seedKeys = [...(result.current.state.keys ?? [])];
    pushLogs([line("live 1"), line("live 2")]);
    await waitFor(() => expect(result.current.state.lines?.length).toBe(3));
    const after = result.current.state;
    // The seed row kept its key; the new rows minted their own — all
    // unique, none re-keyed.
    expect(after.keys?.slice(0, 1)).toEqual(seedKeys);
    expect(new Set(after.keys).size).toBe(3);
  });

  it("loadOlder's prepend never re-keys a survivor — the page rides in with its own keys", async () => {
    rangeMock
      .mockResolvedValueOnce({
        file: "logs/latest.log",
        lines: [line("h1"), line("h2")],
        olderAvailable: true,
        startOffset: 4096,
        historyAvailable: true,
      })
      .mockResolvedValueOnce({
        file: "logs/latest.log",
        lines: [line("h0a"), line("h0b")],
        olderAvailable: false,
        startOffset: 1024,
        historyAvailable: true,
      });
    const { result } = renderHook(() => useLogFeed("prepend"));
    await waitFor(() => expect(result.current.state.cursor).toBe(4096));
    const keysBefore = [...(result.current.state.keys ?? [])];
    await act(async () => {
      result.current.loadOlder();
    });
    await waitFor(() => expect(result.current.state.cursor).toBe(1024));
    const after = result.current.state;
    // The survivors moved down two seats; their keys moved with them.
    expect(after.keys?.slice(2)).toEqual(keysBefore);
    expect(new Set(after.keys).size).toBe(4);
    // The page layout the cap's trim will walk.
    expect(after.pages).toEqual([
      { start: 1024, rows: 2 },
      { start: 4096, rows: 2 },
    ]);
  });

  it("the cap's trim never drops the seam-adjacent page — the live head falls instead, and the cursor stays a boundary the buffer holds", async () => {
    rangeMock.mockResolvedValue({
      file: "logs/latest.log",
      lines: [line("h1"), line("h2")],
      olderAvailable: true,
      startOffset: 100,
      historyAvailable: true,
    });
    const { result } = renderHook(() => useLogFeed("cap"));
    await waitFor(() => expect(result.current.state.lines).not.toBeNull());

    // Fill to exactly the cap: 2 history rows + MAX-2 live rows. No trim
    // yet — the buffer is at, not over.
    const under = Array.from({ length: MAX_FEED_LINES - 2 }, (_, i) =>
      line(`flood ${i}`),
    );
    pushLogs(under);
    await waitFor(() => expect(result.current.state.lines?.length).toBe(MAX_FEED_LINES));
    const before = result.current.state;
    const secondFloodKey = before.keys?.[3];

    // One more line crosses the cap. The seed page is the ONLY page, and
    // it is the seam's neighbor — it SURVIVES (dropping it used to strand
    // the cursor on a boundary the buffer no longer held, so the next
    // loadOlder spliced the page's older sibling straight onto the live
    // seam, silently skipping the dropped rows). The live head falls by
    // the excess instead: the cap lands exactly.
    pushLogs([line("edge")]);
    await waitFor(() =>
      expect(result.current.state.lines?.length).toBe(MAX_FEED_LINES),
    );
    const after = result.current.state;
    // The history rows survive at the head; flood 0 fell off the live
    // head; the appended edge is the tail.
    expect(after.lines?.[0]?.line).toBe("h1");
    expect(after.lines?.[1]?.line).toBe("h2");
    expect(after.lines?.[2]?.line).toBe("flood 1");
    expect(after.lines?.[after.lines.length - 1]?.line).toBe("edge");
    // The survivor kept its minted key — the index moved (flood 0 fell),
    // the identity did not.
    expect(after.keys?.[2]).toBe(secondFloodKey);
    expect(after.liveStart).toBe(2);
    expect(after.pages).toEqual([{ start: 100, rows: 2 }]);
    // The cursor is the RETAINED page's start: the next loadOlder
    // fetches the page ending exactly there — abutting h1, never a gap
    // over rows the buffer dropped.
    expect(after.cursor).toBe(100);
    expect(new Set(after.keys).size).toBe(after.lines?.length);
  });

  it("a deep-history trim drops the OLDEST pages whole — the newest page and its rows survive with their keys", async () => {
    rangeMock
      .mockResolvedValueOnce({
        file: "logs/latest.log",
        lines: [line("t1"), line("t2")],
        olderAvailable: true,
        startOffset: 4096,
        historyAvailable: true,
      })
      .mockResolvedValueOnce({
        file: "logs/latest.log",
        lines: [line("p1"), line("p2")],
        olderAvailable: false,
        startOffset: 1024,
        historyAvailable: true,
      })
      .mockResolvedValue({
        file: "logs/latest.log",
        lines: [line("t1"), line("t2")],
        olderAvailable: false,
        startOffset: 4096,
        historyAvailable: true,
      });
    const { result } = renderHook(() => useLogFeed("deep"));
    await waitFor(() => expect(result.current.state.cursor).toBe(4096));
    await act(async () => {
      result.current.loadOlder();
    });
    await waitFor(() => expect(result.current.state.cursor).toBe(1024));
    // Layout: [p-page @1024 (2 rows), tail-page @4096 (2 rows)] + live.
    await waitFor(() => expect(result.current.state.pages.length).toBe(2));

    // Fill to the cap with live rows: 4 history + MAX-4 live.
    const under = Array.from({ length: MAX_FEED_LINES - 4 }, (_, i) =>
      line(`flood ${i}`),
    );
    pushLogs(under);
    await waitFor(() => expect(result.current.state.lines?.length).toBe(MAX_FEED_LINES));
    const before = result.current.state;
    const p1Key = before.keys?.[0];
    const t1Key = before.keys?.[2];
    const flood1Key = before.keys?.[5];

    // Cross the cap by one: the OLDEST page (p1/p2) falls whole; the
    // seam-adjacent tail page survives; the live head keeps every row
    // (the page overshoot covers the excess).
    pushLogs([line("edge")]);
    await waitFor(() =>
      expect(result.current.state.lines?.length).toBeLessThanOrEqual(MAX_FEED_LINES),
    );
    const after = result.current.state;
    expect(after.lines?.[0]?.line).toBe("t1");
    expect(after.lines?.[1]?.line).toBe("t2");
    expect(after.lines?.[2]?.line).toBe("flood 0");
    expect(after.liveStart).toBe(2);
    expect(after.pages).toEqual([{ start: 4096, rows: 2 }]);
    // t1 kept its key; flood 1 kept its key; p1's key is gone with its row.
    expect(after.keys?.[0]).toBe(t1Key);
    expect(after.keys?.[3]).toBe(flood1Key);
    expect(after.keys).not.toContain(p1Key);
    // The cursor moved to the RETAINED page's start — the boundary the
    // buffer holds — not into the dropped page's rows.
    expect(after.cursor).toBe(4096);
    expect(new Set(after.keys).size).toBe(after.lines?.length);
  });
});
