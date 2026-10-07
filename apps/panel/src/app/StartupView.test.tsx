// StartupView regressions (§38, ADR-0019): the effective values render
// with their provenance words, editing dispatches only what changed,
// clearing dispatches the tri-state null, a non-numeric edit is refused
// locally, and the composed command preview always shows the real launch
// line the daemon will run.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StartupView } from "./StartupView";
import type { ConfigGetResult } from "../protocol/types";

vi.mock("../state/actions", () => ({
  getServerConfig: vi.fn(),
  setServerConfig: vi.fn(),
}));

import { getServerConfig, setServerConfig } from "../state/actions";

const getServerConfigMock = getServerConfig as ReturnType<typeof vi.fn>;
const setServerConfigMock = setServerConfig as ReturnType<typeof vi.fn>;

function configOf(extra: Partial<ConfigGetResult> = {}): ConfigGetResult {
  return {
    serverId: "demo",
    displayName: "Box Demo",
    effective: {
      stopTimeoutSecs: 60,
      startupTimeoutSecs: 120,
      extraJvmArgs: [],
      backupKeep: 10,
    },
    provenance: {
      stopTimeoutSecs: "global",
      startupTimeoutSecs: "global",
      port: "global",
      minMemoryMb: "global",
      maxMemoryMb: "custom",
      extraJvmArgs: "global",
      javaPath: "global",
      mcVersion: "global",
      javaMajorRequired: "global",
      backupKeep: "global",
    },
    ...extra,
  };
}

const baseConfig = configOf({
  jar: "fabric/server.jar",
  effective: {
    stopTimeoutSecs: 60,
    startupTimeoutSecs: 120,
    maxMemoryMb: 2048,
    extraJvmArgs: [],
    backupKeep: 10,
  },
});

beforeEach(() => {
  getServerConfigMock.mockReset().mockResolvedValue(baseConfig);
  setServerConfigMock.mockReset().mockResolvedValue(baseConfig);
});

afterEach(() => {
  cleanup();
});

describe("StartupView", () => {
  it("renders the effective values with provenance and the composed command", async () => {
    render(<StartupView serverId="demo" />);
    const max = await screen.findByLabelText("Maximum memory in MiB");
    expect((max as HTMLInputElement).value).toBe("2048");

    expect(screen.getAllByText("custom").length).toBeGreaterThan(0);
    expect(screen.getAllByText("global").length).toBeGreaterThan(0);
    expect(screen.getByLabelText<HTMLInputElement>("Server JAR").value).toBe(
      "fabric/server.jar",
    );
    // The founder's rule: the actual startup configuration is visible.
    const preview = screen.getByText(/-jar fabric\/server\.jar nogui/);
    expect(preview.textContent).toContain("nogui");
  });

  it("patches only the field that changed", async () => {
    render(<StartupView serverId="demo" />);
    const max = await screen.findByLabelText("Maximum memory in MiB");
    fireEvent.change(max, { target: { value: "4096" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => expect(setServerConfigMock.mock.calls.length).toBe(1));
    const [serverId, payload] = setServerConfigMock.mock.calls[0] as [string, {
      settings: Record<string, unknown>;
      jar?: string | null;
    }];
    expect(serverId).toBe("demo");
    expect(payload.settings).toEqual({ maxMemoryMb: 4096 });
    expect(payload.jar).toBeUndefined();
  });

  it("clearing an override dispatches the tri-state null", async () => {
    render(<StartupView serverId="demo" />);
    await screen.findByLabelText("Maximum memory in MiB");
    const maxRow = screen.getByLabelText("Maximum memory in MiB").closest("div");
    const clear = maxRow?.querySelector("button[aria-label^='Clear']");
    expect(clear).toBeTruthy();
    fireEvent.click(clear as Element);
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => expect(setServerConfigMock.mock.calls.length).toBe(1));
    const [, payload] = setServerConfigMock.mock.calls[0] as [string, {
      settings: Record<string, unknown>;
    }];
    expect(payload.settings).toEqual({ maxMemoryMb: null });
  });

  it("refuses a fractional memory locally, without a request", async () => {
    render(<StartupView serverId="demo" />);
    const max = await screen.findByLabelText("Maximum memory in MiB");
    // jsdom sanitizes non-numeric text out of number inputs; 1.5 rides
    // through and must die at the local integer check.
    fireEvent.change(max, { target: { value: "1.5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("whole number"));
    expect(setServerConfigMock.mock.calls.length).toBe(0);
  });

  it("an unchanged form does not send an empty patch", async () => {
    render(<StartupView serverId="demo" />);
    await screen.findByLabelText("Maximum memory in MiB");
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(setServerConfigMock.mock.calls.length).toBe(0);
  });

  it("daemon refusals surface through describeError", async () => {
    setServerConfigMock.mockRejectedValue(
      new Error("CONFIG_INVALID: the maximum memory setting is invalid"),
    );
    render(<StartupView serverId="demo" />);
    const max = await screen.findByLabelText("Maximum memory in MiB");
    fireEvent.change(max, { target: { value: "4096" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  });
});
