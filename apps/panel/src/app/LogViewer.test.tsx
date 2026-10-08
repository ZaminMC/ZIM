// Log viewer regressions: the file-backed pages seed the view, byte-offset
// paging prepends while keeping the seam honest, level/search filters work,
// and the live-only subscription never duplicates the history it joins.

import { render, screen, cleanup, fireEvent, act, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LogViewer, stripLiveOverlap } from "./LogViewer";

vi.mock("../state/actions", () => ({
  rangeLogs: vi.fn(),
  subscribeLogs: vi.fn(),
}));

import { rangeLogs, subscribeLogs } from "../state/actions";
import type { LogLine, LogLevel, LogRangeResult } from "../protocol/types";

const rangeLogsMock = rangeLogs as ReturnType<typeof vi.fn>;
const subscribeLogsMock = subscribeLogs as ReturnType<typeof vi.fn>;

type LivePayload = { payload: { kind: string; batch?: LogLine[] } };
type PayloadHandler = (notification: LivePayload) => void;

/** Capture the stream handler's onPayload so tests push batches manually. */
let liveHandler: PayloadHandler | null = null;

function page(startOffset: number, lines: string[], olderAvailable = true): LogRangeResult {
  return {
    file: "logs/latest.log",
    lines: lines.map((line) => ({ tsMs: 0, level: "info", thread: "Server thread", line })),
    olderAvailable,
    startOffset,
    historyAvailable: true,
  };
}

function linesOf(...texts: string[]): LogLine[] {
  return texts.map((line) => ({ tsMs: 0, level: "info", thread: "Server thread", line }));
}

function lineOf(level: LogLevel, text: string): LogLine {
  return { tsMs: 0, level, thread: "Server thread", line: text };
}

beforeEach(() => {
  liveHandler = null;
  subscribeLogsMock.mockImplementation(
    (_serverId: string, handler: { onPayload: PayloadHandler }) => {
      liveHandler = handler.onPayload;
      return Promise.resolve({ dispose: vi.fn(), result: null });
    },
  );
  rangeLogsMock.mockReset();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("stripLiveOverlap", () => {
  it("drops the live prefix that duplicates the history suffix", () => {
    const history = linesOf("a", "b", "c");
    const live = [...linesOf("b", "c"), lineOf("warn", "d")];
    expect(stripLiveOverlap(history, live).map((l) => l.line)).toEqual(["d"]);
  });

  it("keeps everything when there is no matching suffix", () => {
    const history = linesOf("a", "b");
    const live = linesOf("x", "y");
    expect(stripLiveOverlap(history, live)).toHaveLength(2);
  });

  it("matches on line text only (levels and tsMs differ across sources)", () => {
    const history = [{ tsMs: 0, level: "info", line: "same" } as LogLine];
    const live = [{ tsMs: 1730, level: "warn", line: "same" } as LogLine];
    expect(stripLiveOverlap(history, live)).toHaveLength(0);
  });
});

describe("LogViewer", () => {
  it("seeds the tail, marks the seam, and filters by level and search", async () => {
    rangeLogsMock.mockResolvedValue({
      file: "logs/latest.log",
      lines: [
        lineOf("info", "[INFO] ready line"),
        lineOf("warn", "[WARN] watch out"),
        lineOf("error", "[ERROR] kaput"),
      ],
      olderAvailable: true,
      startOffset: 400,
      historyAvailable: true,
    });
    render(<LogViewer serverId="smp" />);

    await waitFor(() => expect(screen.getByText("[ERROR] kaput")).toBeTruthy());

    // Level filter.
    fireEvent.click(screen.getByRole("button", { name: "Error" }));
    expect(screen.queryByText("[INFO] ready line")).toBeNull();
    expect(screen.getByText("[ERROR] kaput")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "All" }));

    // Search (thread + text haystack).
    fireEvent.change(screen.getByLabelText("Search log lines"), {
      target: { value: "kaput" },
    });
    expect(screen.queryByText("[INFO] ready line")).toBeNull();
    expect(screen.getByText("[ERROR] kaput")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Search log lines"), { target: { value: "" } });
  });

  it("pages older lines in above the seam and reports exhaustion", async () => {
    rangeLogsMock.mockResolvedValueOnce(page(400, ["line 3", "line 4"]));
    render(<LogViewer serverId="smp" />);
    await waitFor(() => expect(screen.getByText("line 4")).toBeTruthy());

    rangeLogsMock.mockResolvedValueOnce(page(0, ["line 1", "line 2"], false));
    fireEvent.click(screen.getByRole("button", { name: "Load older lines" }));

    await waitFor(() => expect(screen.getByText("line 1")).toBeTruthy());
    expect(screen.getByText("line 2")).toBeTruthy();
    expect(screen.getByText("line 3")).toBeTruthy();
    expect(screen.getByText("beginning of the log file")).toBeTruthy();
    // The tail read (first) plus the page (second): the cursor came from
    // the first page's startOffset.
    expect(rangeLogsMock).toHaveBeenLastCalledWith({
      serverId: "smp",
      maxLines: 200,
      beforeOffset: 400,
    });
  });

  it("joins live lines after the seam without duplicating the overlap", async () => {
    rangeLogsMock.mockResolvedValue(page(400, ["tail one", "tail two"]));
    render(<LogViewer serverId="smp" />);

    // The overlap window: lines ingested while both the subscription and
    // the tail read were starting — buffered pre-seam, stripped against
    // the seeded history.
    await act(async () => {
      liveHandler?.({
        payload: { kind: "logs", batch: [...linesOf("tail two"), ...linesOf("fresh line")] },
      });
    });
    await waitFor(() => expect(screen.getByText("tail two")).toBeTruthy());

    // Exactly one "tail two": the history's. The seam marker rides above
    // the first genuinely live row.
    expect(screen.getAllByText("tail two")).toHaveLength(1);
    expect(screen.getByText("— live —")).toBeTruthy();
    expect(screen.getByText("fresh line")).toBeTruthy();

    // Post-seam batches append verbatim; other payload kinds are ignored.
    act(() => {
      liveHandler?.({ payload: { kind: "logs", batch: linesOf("second live") } });
      liveHandler?.({ payload: { kind: "missed" } });
    });
    expect(screen.getByText("second live")).toBeTruthy();
  });

  it("shows the typed error with a retry when the log file is missing", async () => {
    rangeLogsMock.mockRejectedValue({
      code: "FS_NOT_FOUND",
      message: "no log file yet",
      remediation: [],
    });
    render(<LogViewer serverId="fresh" />);
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.getByText(/FS_NOT_FOUND/)).toBeTruthy();

    // Retry succeeds: the history appears, the error clears.
    rangeLogsMock.mockResolvedValueOnce(page(0, ["recovered line"], false));
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getByText("recovered line")).toBeTruthy());
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
