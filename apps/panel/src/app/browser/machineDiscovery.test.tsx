// The new tab's machine section (§64, ADR-0027): the slug rules, the
// honest section behavior (hidden when the scan has nothing, budget truth
// stated, skipped roots named), and the open verb — register then
// navigate, with a refusal rendered as a typed note and nothing
// half-opened.

import { render, screen, cleanup, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MachineDiscovery, slugFromPath } from "./machineDiscovery";
import type { DiscoveredServer } from "../../protocol/types";

vi.mock("../../state/actions", () => ({
  discoverServers: vi.fn(),
}));

import { discoverServers } from "../../state/actions";
const discoverMock = discoverServers as ReturnType<typeof vi.fn>;

function candidateOf(over: Partial<DiscoveredServer>): DiscoveredServer {
  return {
    path: "/srv/minecraft/survival",
    kind: "directory",
    displayName: "survival",
    port: 25565,
    ...over,
  };
}

beforeEach(() => {
  discoverMock.mockReset().mockResolvedValue({
    servers: [],
    roots: [],
    skippedRoots: [],
    scanned: 0,
    truncated: false,
  });
});

afterEach(cleanup);

describe("slugFromPath", () => {
  it("mirrors the daemon's ServerId rules", () => {
    expect(slugFromPath("/srv/mc/My Server 2!")).toBe("my-server-2");
    expect(slugFromPath("C:\\boxes\\Paper_1.21")).toBe("paper_1-21");
    expect(slugFromPath("/srv/mc/plain")).toBe("plain");
  });

  it("never hands back an empty or symbol-leading id", () => {
    expect(slugFromPath("/srv/mc/***")).toBe("s-");
    expect(slugFromPath("/")).toBe("server");
  });
});

describe("MachineDiscovery", () => {
  it("renders nothing when the scan answers nothing", async () => {
    render(<MachineDiscovery query="" onOpen={() => {}} />);
    await waitFor(() => expect(discoverMock).toHaveBeenCalled());
    expect(screen.queryByText("On this machine")).toBeNull();
  });

  it("lists directories and jars with their evidence, registered rows never shown", async () => {
    discoverMock.mockResolvedValue({
      servers: [
        candidateOf({}),
        candidateOf({
          kind: "registered",
          serverId: "managed",
          displayName: "Managed",
          state: "running",
        }),
        candidateOf({
          kind: "jar",
          path: "/downloads/paper-1.21.4.jar",
          displayName: undefined,
          jarName: "paper-1.21.4.jar",
          platform: "paper",
        }),
      ],
      roots: ["/srv/minecraft"],
      skippedRoots: [],
      scanned: 24,
      truncated: false,
    });
    render(<MachineDiscovery query="" onOpen={() => {}} />);
    await waitFor(() => expect(screen.getByText("On this machine")).toBeTruthy());
    expect(screen.getByText("survival")).toBeTruthy();
    expect(screen.getByText("paper-1.21.4.jar")).toBeTruthy();
    expect(screen.getByText(/folder/)).toBeTruthy();
    expect(screen.queryByText("Managed")).toBeNull();
  });

  it("states the budget truth and names skipped roots", async () => {
    discoverMock.mockResolvedValue({
      servers: [candidateOf({})],
      roots: ["/srv/minecraft"],
      skippedRoots: ["/mnt/nas/gone"],
      scanned: 4096,
      truncated: true,
    });
    render(<MachineDiscovery query="" onOpen={() => {}} />);
    await waitFor(() => expect(screen.getByText(/hit its budget/)).toBeTruthy());
    expect(screen.getByText(/\/mnt\/nas\/gone/)).toBeTruthy();
  });

  it("renders a refused scan as a typed alert, not a fake empty", async () => {
    discoverMock.mockRejectedValue(new Error("the daemon is unreachable"));
    render(<MachineDiscovery query="" onOpen={() => {}} />);
    await waitFor(() =>
      expect(screen.getByText(/could not run/)).toBeTruthy(),
    );
    expect(screen.getByText(/unreachable/)).toBeTruthy();
  });

  it("hands the candidate to onOpen on a row click", async () => {
    const onOpen = vi.fn();
    const candidate = candidateOf({});
    discoverMock.mockResolvedValue({
      servers: [candidate],
      roots: [],
      skippedRoots: [],
      scanned: 1,
      truncated: false,
    });
    render(<MachineDiscovery query="" onOpen={onOpen} />);
    await waitFor(() => screen.getByText("survival"));
    screen.getByText("survival").click();
    expect(onOpen).toHaveBeenCalledWith(candidate);
  });
});
