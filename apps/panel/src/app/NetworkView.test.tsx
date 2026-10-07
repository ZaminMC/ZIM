// NetworkView regressions (§37, ADR-0019): the probe dot answers honestly
// (available / in use / nothing to probe), the properties authority and
// the bind address render read-only, conflicts are named, the port save
// patches the config model, and the daemon's conflict list reaches the UI.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NetworkView } from "./NetworkView";
import type { ConfigGetResult, NetworkStatusResult } from "../protocol/types";

vi.mock("../state/actions", () => ({
  getServerConfig: vi.fn(),
  setServerConfig: vi.fn(),
  getNetworkStatus: vi.fn(),
}));

import { getNetworkStatus, getServerConfig, setServerConfig } from "../state/actions";

const getServerConfigMock = getServerConfig as ReturnType<typeof vi.fn>;
const getNetworkStatusMock = getNetworkStatus as ReturnType<typeof vi.fn>;
const setServerConfigMock = setServerConfig as ReturnType<typeof vi.fn>;

const config: ConfigGetResult = {
  serverId: "alpha",
  displayName: "Alpha",
  effective: {
    stopTimeoutSecs: 60,
    startupTimeoutSecs: 120,
    port: 25580,
    extraJvmArgs: [],
    backupKeep: 10,
  },
  provenance: {
    stopTimeoutSecs: "global",
    startupTimeoutSecs: "global",
    port: "custom",
    minMemoryMb: "global",
    maxMemoryMb: "global",
    extraJvmArgs: "global",
    javaPath: "global",
    mcVersion: "global",
    javaMajorRequired: "global",
    backupKeep: "global",
  },
};

function statusOf(extra: Partial<NetworkStatusResult> = {}): NetworkStatusResult {
  return {
    serverId: "alpha",
    desiredPort: 25580,
    propertiesPort: 25580,
    bindAddress: "127.0.0.1",
    portAvailable: true,
    conflicts: ["beta"],
    ...extra,
  };
}

beforeEach(() => {
  getServerConfigMock.mockReset().mockResolvedValue(config);
  getNetworkStatusMock.mockReset().mockResolvedValue(statusOf());
  setServerConfigMock.mockReset().mockResolvedValue(config);
});

afterEach(() => {
  cleanup();
});

describe("NetworkView", () => {
  it("renders the desired port, the boot authority, and the probe", async () => {
    render(<NetworkView serverId="alpha" />);
    await waitFor(() => expect(screen.getByText(/available right now/)).toBeTruthy());
    expect(screen.getByText("25580")).toBeTruthy();
    expect(screen.getByText("127.0.0.1")).toBeTruthy();
    expect(screen.getByLabelText<HTMLInputElement>("Minecraft port").value).toBe("25580");
  });

  it("names the conflict and offers the re-probe", async () => {
    render(<NetworkView serverId="alpha" />);
    await waitFor(() => expect(screen.getByText(/⚠ beta/)).toBeTruthy());

    getNetworkStatusMock.mockResolvedValue(statusOf({ portAvailable: false, conflicts: [] }));
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    await waitFor(() => expect(screen.getByText(/in use right now/)).toBeTruthy());
    expect(screen.queryByText(/⚠ beta/)).toBeNull();
  });

  it("saves a port change as a config patch and re-probes", async () => {
    render(<NetworkView serverId="alpha" />);
    const port = await screen.findByLabelText("Minecraft port");
    fireEvent.change(port, { target: { value: "25590" } });
    fireEvent.click(screen.getByRole("button", { name: "Save port" }));

    await waitFor(() => expect(setServerConfigMock.mock.calls.length).toBe(1));
    const [serverId, payload] = setServerConfigMock.mock.calls[0] as [string, {
      settings: Record<string, unknown>;
    }];
    expect(serverId).toBe("alpha");
    expect(payload).toEqual({ settings: { port: 25590 } });
    await waitFor(() => expect(getNetworkStatusMock.mock.calls.length).toBe(2));
  });

  it("an in-use port is stated, not hidden", async () => {
    getNetworkStatusMock.mockResolvedValue(statusOf({ portAvailable: false }));
    render(<NetworkView serverId="alpha" />);
    await waitFor(() => expect(screen.getByText(/in use right now/)).toBeTruthy());
  });

  it("no port anywhere means no probe, stated plainly", async () => {
    getServerConfigMock.mockResolvedValue({
      ...config,
      effective: { ...config.effective, port: undefined },
    });
    getNetworkStatusMock.mockResolvedValue(
      statusOf({ desiredPort: undefined, portAvailable: undefined }),
    );
    render(<NetworkView serverId="alpha" />);
    await waitFor(() => expect(screen.getByText(/no port to probe/)).toBeTruthy());
  });

  it("a daemon refusal surfaces as the alert, not a silent no-op", async () => {
    setServerConfigMock.mockRejectedValue(new Error("CONFIG_INVALID: the port"));
    render(<NetworkView serverId="alpha" />);
    const port = await screen.findByLabelText("Minecraft port");
    fireEvent.change(port, { target: { value: "80" } });
    fireEvent.click(screen.getByRole("button", { name: "Save port" }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  });
});
