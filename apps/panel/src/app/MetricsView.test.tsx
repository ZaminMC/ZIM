// Metrics tab regressions (§31): the rows read the store alone (opening
// the tab never waits on a fresh subscribe), every allowance is a real
// number from its owner — the config model's heap, the ping's
// max-players, the host's cores — and an unknown ceiling renders the row
// without one instead of inventing it.

import { render, screen, cleanup, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MetricsView } from "./MetricsView";

vi.mock("../state/metrics", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../state/metrics")>()),
  useServerMetrics: vi.fn(),
}));

vi.mock("../state/actions", () => ({
  getServerConfig: vi.fn(),
  listPlayers: vi.fn(),
}));

import { useServerMetrics } from "../state/metrics";
import { getServerConfig, listPlayers } from "../state/actions";

const useServerMetricsMock = useServerMetrics as unknown as ReturnType<typeof vi.fn>;
const getServerConfigMock = getServerConfig as ReturnType<typeof vi.fn>;
const listPlayersMock = listPlayers as ReturnType<typeof vi.fn>;

function sample(extra: Record<string, number> = {}) {
  return {
    tsMs: 1_730_803_200_000,
    cpuPercent: 12.5,
    rssBytes: 1024 * 1024 * 512,
    players: 3,
    uptimeMs: 65_000,
    ...extra,
  };
}

beforeEach(() => {
  useServerMetricsMock.mockReset().mockReturnValue([]);
  getServerConfigMock.mockReset().mockRejectedValue(new Error("no wire"));
  listPlayersMock.mockReset().mockRejectedValue(new Error("no wire"));
});

afterEach(cleanup);

describe("MetricsView", () => {
  it("says honestly that it has nothing when no samples have arrived", () => {
    useServerMetricsMock.mockReturnValue([]);
    render(<MetricsView serverId="smp" />);
    expect(screen.getByText(/No samples yet/)).toBeTruthy();
  });

  it("renders the current/allowed rows when the ceilings are known", async () => {
    useServerMetricsMock.mockReturnValue([sample()]);
    getServerConfigMock.mockResolvedValue({
      serverId: "smp",
      displayName: "SMP",
      effective: { maxMemoryMb: 4096 },
      provenance: {},
    });
    listPlayersMock.mockResolvedValue({ source: "ping", online: 3, max: 20, sample: [], latencyMs: 5 });

    render(<MetricsView serverId="smp" />);
    await waitFor(() => expect(screen.getByText(/4.0 GB/)).toBeTruthy());
    await waitFor(() => expect(screen.getByText("/ 20")).toBeTruthy());
    // CPU against the jsdom host's cores.
    await waitFor(() => expect(screen.getByText("/ 200%")).toBeTruthy()); // jsdom: 2 cores
    // The meters exist and are honest about their share.
    const meters = screen.getAllByRole("meter");
    expect(meters.length).toBe(3);
    expect(meters[1].getAttribute("aria-valuenow")).toBe("13"); // 512 MiB of 4 GiB
    expect(meters[2].getAttribute("aria-valuenow")).toBe("15"); // 3 of 20
  });

  it("renders without a ceiling instead of inventing one", () => {
    useServerMetricsMock.mockReturnValue([sample()]);
    // Both allowance calls fail (the beforeEach rejects) — the rows must
    // still render, with no " / undefined" anywhere.
    render(<MetricsView serverId="smp" />);
    // The number appears in the row and again in the chart head.
    expect(screen.getAllByText("12.5%").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("3").length).toBeGreaterThanOrEqual(1);
    expect(screen.queryByText(/undefined/)).toBeNull();
    // Whatever ceilings the host does not own (here: the heap, the ping)
    // render no meter track — the CPU meter exists only because jsdom
    // publishes a real hardwareConcurrency.
    const labels = screen.queryAllByRole("meter").map((m) => m.getAttribute("aria-label"));
    expect(labels.every((label) => label?.startsWith("CPU"))).toBe(true);
  });

  it("keeps the sparklines fed from the store window", () => {
    useServerMetricsMock.mockReturnValue([
      sample({ cpuPercent: 5, rssBytes: 1_000_000 }),
      sample({ cpuPercent: 40, rssBytes: 2_000_000 }),
    ]);
    render(<MetricsView serverId="smp" />);
    // One polyline per chart (CPU + RSS) is fed from the window.
    expect(document.querySelectorAll("polyline").length).toBe(2);
  });
});
