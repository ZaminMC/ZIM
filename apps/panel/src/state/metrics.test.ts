// The metrics store: bounded window, timestamp dedup (ring replay vs.
// live tick), seed-then-live merge, and reference-counted stream wiring.

import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ensureMetrics,
  forgetMetrics,
  formatBytes,
  formatCpu,
  formatUptime,
  METRICS_WINDOW,
  releaseMetrics,
  useMetrics,
} from "./metrics";
import { subscribeMetrics, metricsRange } from "./actions";
import type { MetricsSample } from "../protocol/types";

vi.mock("./actions", () => ({
  metricsRange: vi.fn(() => Promise.resolve({ samples: [] })),
  subscribeMetrics: vi.fn(() => Promise.resolve({ dispose: vi.fn() })),
}));
vi.mock("../logger", () => ({ logWarn: vi.fn() }));

vi.mock("./wire", () => ({ client: {} }));

function sample(tsMs: number): MetricsSample {
  return { tsMs, cpuPercent: 1 + tsMs * 0.01, rssBytes: 1000 + tsMs };
}

beforeEach(() => {
  vi.mocked(metricsRange).mockClear();
  vi.mocked(subscribeMetrics).mockClear();
  useMetrics.setState({ samples: {} });
});

describe("metrics store", () => {
  it("appends samples, dedupes by timestamp, and caps the window", () => {
    const { append } = useMetrics.getState();
    append("s", sample(2));
    append("s", sample(1)); // older than the last tick: dropped
    append("s", sample(2)); // same tick (replay): dropped
    append("s", sample(3));
    let history = useMetrics.getState().samples.s ?? [];
    expect(history.map((s) => s.tsMs)).toEqual([2, 3]);

    for (let ts = 4; ts < 4 + METRICS_WINDOW + 10; ts += 1) {
      append("s", sample(ts));
    }
    history = useMetrics.getState().samples.s ?? [];
    expect(history.length).toBe(METRICS_WINDOW);
    expect(history[0]?.tsMs).toBe(4 + 10); // newest kept, oldest trimmed
  });

  it("seeds range history and keeps live samples that are newer", () => {
    const { seed, append } = useMetrics.getState();
    append("s", sample(50)); // a live tick landed first
    seed("s", [sample(10), sample(20)]);
    const history = useMetrics.getState().samples.s ?? [];
    expect(history.map((s) => s.tsMs)).toEqual([10, 20, 50]);
  });

  it("reference-counts one subscription per server", async () => {
    const dispose = vi.fn();
    vi.mocked(subscribeMetrics).mockReturnValue(Promise.resolve({ dispose }));
    const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

    ensureMetrics("s");
    ensureMetrics("s"); // second joiner shares the stream
    expect(subscribeMetrics).toHaveBeenCalledTimes(1);
    await flush(); // the disposer lands asynchronously

    releaseMetrics("s"); // one reference left: still open
    expect(dispose).not.toHaveBeenCalled();

    releaseMetrics("s"); // last reference: closed
    expect(dispose).toHaveBeenCalledTimes(1);

    ensureMetrics("s"); // reopening subscribes fresh
    expect(subscribeMetrics).toHaveBeenCalledTimes(2);
  });

  it("forget clears one server's history", () => {
    const { append } = useMetrics.getState();
    append("a", sample(1));
    append("b", sample(1));
    forgetMetrics("a");
    expect(useMetrics.getState().samples.a).toBeUndefined();
    expect(useMetrics.getState().samples.b).toHaveLength(1);
  });
});

describe("metric formatters", () => {
  it("bytes climb units honestly and answer — when unmeasured", () => {
    expect(formatBytes(undefined)).toBe("—");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(1536 * 1024 * 1024)).toBe("1.5 GB");
  });

  it("cpu and uptime keep tabular shapes", () => {
    expect(formatCpu(undefined)).toBe("—");
    expect(formatCpu(12.34)).toBe("12.3%");
    expect(formatCpu(120.4)).toBe("120%"); // multicore can exceed 100
    expect(formatUptime(undefined)).toBe("—");
    expect(formatUptime(0)).toBe("0:00");
    expect(formatUptime(65_000)).toBe("1:05");
    expect(formatUptime(3_725_000)).toBe("1:02:05");
  });
});
