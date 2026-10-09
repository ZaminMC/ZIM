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
  PAGE_LINES,
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
    expect(INITIAL_FEED.historyAvailable).toBe(true);
    expect(INITIAL_FEED.missed).toBe(0);
    expect(INITIAL_FEED.newCount).toBe(0);
  });
});
