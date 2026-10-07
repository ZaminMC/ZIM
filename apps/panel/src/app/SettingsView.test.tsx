// SettingsView regressions (§39, ADR-0019): identity edits reach the wire
// and refresh the panel's server record through the ordinary read path,
// reserved rooms are stated rather than faked (§82), and local validation
// refuses a blank name or a nonsense retention without a request.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsView } from "./SettingsView";
import type { ConfigGetResult, ServerDetails } from "../protocol/types";

vi.mock("../state/actions", () => ({
  getServerConfig: vi.fn(),
  setServerConfig: vi.fn(),
  getServer: vi.fn(),
}));

vi.mock("../state/servers", () => ({
  useServers: (selector: (s: unknown) => unknown) => selector({ upsert: upsertSpy }),
}));

import { getServer, getServerConfig, setServerConfig } from "../state/actions";

const getServerConfigMock = getServerConfig as ReturnType<typeof vi.fn>;
const setServerConfigMock = setServerConfig as ReturnType<typeof vi.fn>;
const getServerMock = getServer as ReturnType<typeof vi.fn>;
const upsertSpy = vi.fn();

const config: ConfigGetResult = {
  serverId: "demo",
  displayName: "Box Demo",
  effective: {
    stopTimeoutSecs: 60,
    startupTimeoutSecs: 120,
    port: 25565,
    extraJvmArgs: [],
    backupKeep: 10,
  },
  provenance: {
    stopTimeoutSecs: "global",
    startupTimeoutSecs: "global",
    port: "global",
    minMemoryMb: "global",
    maxMemoryMb: "global",
    extraJvmArgs: "global",
    javaPath: "global",
    mcVersion: "global",
    javaMajorRequired: "global",
    backupKeep: "global",
  },
};

const details: ServerDetails = {
  serverId: "demo",
  displayName: "Box Demo",
  state: "not-running",
};

beforeEach(() => {
  getServerConfigMock.mockReset().mockResolvedValue(config);
  setServerConfigMock.mockReset().mockResolvedValue(config);
  getServerMock.mockReset().mockResolvedValue(details);
  upsertSpy.mockReset();
});

afterEach(() => {
  cleanup();
});

describe("SettingsView", () => {
  it("renders the identity, the join address, and the retention", async () => {
    render(<SettingsView serverId="demo" />);
    await waitFor(() =>
      expect(screen.getByLabelText<HTMLInputElement>("Server name").value).toBe("Box Demo"),
    );
    expect(screen.getByText("0.0.0.0:25565")).toBeTruthy();
    expect(screen.getByLabelText<HTMLInputElement>("Backup retention count").value).toBe("10");
  });

  it("patches the name and the retention together, then refreshes the store", async () => {
    render(<SettingsView serverId="demo" />);
    fireEvent.change(await screen.findByLabelText("Server name"), {
      target: { value: "Survival" },
    });
    fireEvent.change(screen.getByLabelText("Backup retention count"), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => expect(setServerConfigMock.mock.calls.length).toBe(1));
    const [serverId, payload] = setServerConfigMock.mock.calls[0] as [string, {
      displayName?: string;
      settings: Record<string, unknown>;
    }];
    expect(serverId).toBe("demo");
    expect(payload).toEqual({
      displayName: "Survival",
      settings: { backupKeep: 5 },
    });
    await waitFor(() => expect(upsertSpy.mock.calls.length).toBe(1));
  });

  it("a blank name is refused locally, without a request", async () => {
    render(<SettingsView serverId="demo" />);
    fireEvent.change(await screen.findByLabelText("Server name"), {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("must not be empty"));
    expect(setServerConfigMock.mock.calls.length).toBe(0);
  });

  it("a nonsense retention is refused locally", async () => {
    render(<SettingsView serverId="demo" />);
    fireEvent.change(await screen.findByLabelText("Backup retention count"), {
      target: { value: "0" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("whole number"));
    expect(setServerConfigMock.mock.calls.length).toBe(0);
  });

  it("reserved rooms are stated, never faked (§82)", async () => {
    render(<SettingsView serverId="demo" />);
    await screen.findByLabelText("Server name");
    expect(screen.getAllByText("reserved").length).toBe(4); // icon, restart, crash, logs
    expect(screen.getByText(/icon picker/)).toBeTruthy();
    expect(screen.getByText(/Restart policy/)).toBeTruthy();
    expect(screen.getByText(/Crash policy/)).toBeTruthy();
    expect(screen.getByText(/Log retention/)).toBeTruthy();
  });

  it("an unchanged form keeps the save disabled", async () => {
    render(<SettingsView serverId="demo" />);
    await screen.findByLabelText("Server name");
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Save changes" }).disabled,
    ).toBe(true);
  });
});
