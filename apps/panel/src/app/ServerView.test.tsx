// ServerView: buttons follow the ADR-0005 ladder, errors render with
// remediation, crash cards surface the daemon's classification.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ServerView, availableVerbs } from "./ServerView";
import { ProtocolRequestError } from "../protocol/client";
import { useServers } from "../state/servers";
import { useMetrics } from "../state/metrics";
import { useUi } from "../state/ui";
import { useTabs } from "../state/tabs";

const mocks = vi.hoisted(() => ({
  getServer: vi.fn(),
  startServer: vi.fn(),
  stopServer: vi.fn(),
  restartServer: vi.fn(),
  killServer: vi.fn(),
  writeWholeFile: vi.fn(),
  consoleRenders: 0,
}));

vi.mock("../state/wire", () => ({ client: {}, startWire: vi.fn() }));
vi.mock("./ConsoleView", () => ({
  ConsoleView: () => {
    mocks.consoleRenders += 1;
    return <div data-testid="console-stub" />;
  },
}));
vi.mock("../state/actions", () => ({
  getServer: mocks.getServer,
  startServer: mocks.startServer,
  stopServer: mocks.stopServer,
  restartServer: mocks.restartServer,
  killServer: mocks.killServer,
  writeWholeFile: mocks.writeWholeFile,
  // The metrics workspace stream is fire-and-forget background wiring;
  // tests never exercise it here.
  metricsRange: vi.fn(() => Promise.resolve({ samples: [] })),
  subscribeMetrics: vi.fn(() => Promise.resolve({ dispose: () => {} })),
}));

describe("availableVerbs", () => {
  it("follows the lifecycle ladder", () => {
    expect(availableVerbs("running")).toEqual(["stop", "restart", "kill"]);
    expect(availableVerbs("starting")).toEqual(["stop", "restart", "kill"]);
    expect(availableVerbs("not-running")).toEqual(["start"]);
    expect(availableVerbs("crashed")).toEqual(["start"]);
    expect(availableVerbs("failed-preflight")).toEqual(["start"]);
  });
});

describe("ServerView", () => {
  beforeEach(() => {
    useServers.setState({
      servers: {
        alpha: { serverId: "alpha", displayName: "Alpha", state: "running" },
      },
      crashes: {},
    });
    useUi.setState({ pending: {} });
    useTabs.setState({
      tabs: [{ id: "t-alpha", history: [{ kind: "server", serverId: "alpha" }], historyIndex: 0, reloadToken: 0 }],
      activeId: "t-alpha",
      discoveryQuery: null,
    });
    useMetrics.setState({ samples: {} });
    mocks.consoleRenders = 0;

    mocks.getServer.mockResolvedValue({
      serverId: "alpha",
      displayName: "Alpha",
      state: "running",
      software: "Paper",
      version: "1.21",
      port: 25565,
    });
    mocks.startServer.mockRejectedValue(
      new ProtocolRequestError({
        code: "SERVER_ALREADY_RUNNING",
        message: "The server is already running.",
      }),
    );
    mocks.stopServer.mockRejectedValue(
      new ProtocolRequestError({
        code: "SERVER_STOP_TIMEOUT",
        message: "The stop ladder timed out.",
        remediation: ["Force kill from the Kill button."],
      }),
    );
    mocks.restartServer.mockResolvedValue({ serverId: "alpha", state: "stopping" });
    mocks.killServer.mockResolvedValue({ serverId: "alpha", state: "stopping" });
  });

  afterEach(cleanup);

  it("renders header facts and state-appropriate buttons", async () => {
    render(<ServerView serverId="alpha" />);
    expect(screen.getByText("Alpha")).toBeTruthy();
    await screen.findByText("Paper"); // details arrive via server.get
    expect(screen.getByText(":25565")).toBeTruthy();

    const start = screen.getByRole("button", { name: "Start" });
    const stop = screen.getByRole("button", { name: "Stop" });
    expect((start as HTMLButtonElement).disabled).toBe(true); // already running
    expect((stop as HTMLButtonElement).disabled).toBe(false);
  });

  it("disables everything while a verb is in flight for that server", () => {
    useUi.setState({ pending: { alpha: "stop" } });
    render(<ServerView serverId="alpha" />);
    const stop = screen.getByRole("button", { name: "Stop" });
    expect((stop as HTMLButtonElement).disabled).toBe(true);
  });

  it("shows a structured action error with remediation", async () => {
    render(<ServerView serverId="alpha" />);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    await screen.findByRole("alert");
    expect(screen.getByText(/stop ladder timed out/)).toBeTruthy();
    expect(screen.getByText("SERVER_STOP_TIMEOUT")).toBeTruthy();
    expect(screen.getByText("Force kill from the Kill button.")).toBeTruthy();
    // Dismiss works.
    fireEvent.click(screen.getByRole("button", { name: "Dismiss error" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("renders the crash card with phase, exit code and evidence", () => {
    useServers.setState({
      crashes: {
        alpha: {
          serverId: "alpha",
          phase: "startup",
          exitCode: 3,
          evidence: 'Exception in thread "main" java.lang.RuntimeException',
          resolved: false,
        },
      },
    });
    render(<ServerView serverId="alpha" />);
    expect(screen.getByText("Server stopped unexpectedly.")).toBeTruthy();
    expect(screen.getByText(/during startup/)).toBeTruthy();
    expect(screen.getByText(/exit code: 3/)).toBeTruthy();
    expect(screen.getByText(/Exception in thread/)).toBeTruthy();
  });

  it("dismisses the crash card on dismiss", async () => {
    useServers.setState({
      crashes: {
        alpha: { serverId: "alpha", phase: "runtime", exitCode: 1, resolved: false },
      },
    });
    render(<ServerView serverId="alpha" />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss crash report" }));
    await waitFor(() => expect(useServers.getState().crashes["alpha"]?.resolved).toBe(true));
  });

  it("a metrics flood re-renders the header chips, never the workspace", async () => {
    render(<ServerView serverId="alpha" />);
    await screen.findByTestId("console-stub"); // the lazy console resolved
    const rendersBefore = mocks.consoleRenders;
    expect(rendersBefore).toBeGreaterThan(0);

    // Three 1 Hz samples land on the metrics store (the stream wiring is
    // mocked; the store and the chips are real).
    useMetrics.getState().append("alpha", { tsMs: 1, cpuPercent: 3, rssBytes: 100 });
    useMetrics.getState().append("alpha", { tsMs: 2, cpuPercent: 4, rssBytes: 101 });
    useMetrics.getState().append("alpha", { tsMs: 3, cpuPercent: 5, rssBytes: 102 });

    expect(await screen.findByText("5.0%")).toBeTruthy();
    expect(screen.getByText("102 B")).toBeTruthy();
    // The workspace surface did not re-render for background data — the
    // panel's "interaction → next paint < 100 ms under load" budget rests
    // on this isolation.
    expect(mocks.consoleRenders).toBe(rendersBefore);
  });

  it("hides the live chips once the server is no longer live", async () => {
    useServers.setState({
      servers: {
        alpha: { serverId: "alpha", displayName: "Alpha", state: "stopped" },
      },
    });
    // server.get must agree, or its upsert would flip the state back.
    mocks.getServer.mockResolvedValue({
      serverId: "alpha",
      displayName: "Alpha",
      state: "stopped",
    });
    render(<ServerView serverId="alpha" />);
    useMetrics.getState().append("alpha", { tsMs: 1, cpuPercent: 3, rssBytes: 100 });
    await screen.findByTestId("console-stub");
    // Stale samples never masquerade as live numbers.
    expect(screen.queryByText("3.0%")).toBeNull();
  });
});

describe("ServerView — EULA acceptance", () => {
  beforeEach(() => {
    useServers.setState({
      servers: {
        fresh: { serverId: "fresh", displayName: "Fresh", state: "not-running" },
      },
      crashes: {},
    });
    useUi.setState({ pending: {} });
    useTabs.setState({
      tabs: [{ id: "t-fresh", history: [{ kind: "server", serverId: "fresh" }], historyIndex: 0, reloadToken: 0 }],
      activeId: "t-fresh",
      discoveryQuery: null,
    });
    mocks.getServer.mockReset().mockResolvedValue({
      serverId: "fresh",
      displayName: "Fresh",
      state: "not-running",
    });
  });

  afterEach(cleanup);

  it("turns the typed NEEDS_EULA into an accept-and-start affordance", async () => {
    mocks.startServer.mockReset();
    mocks.startServer
      .mockRejectedValueOnce(
        new ProtocolRequestError({
          code: "NEEDS_EULA",
          message: "The server cannot start: EULA is not accepted.",
          remediation: ["accept_eula"],
        }),
      )
      .mockResolvedValueOnce({ serverId: "fresh", state: "starting" });
    mocks.writeWholeFile.mockReset().mockResolvedValue(undefined);

    render(<ServerView serverId="fresh" />);
    const start = await screen.findByRole("button", { name: "Start" });
    fireEvent.click(start);

    const accept = await screen.findByRole("button", { name: /Accept EULA & start/i });
    fireEvent.click(accept);

    await waitFor(() => expect(mocks.writeWholeFile).toHaveBeenCalled());
    expect(mocks.writeWholeFile.mock.calls[0]?.[0]).toBe("fresh");
    expect(mocks.writeWholeFile.mock.calls[0]?.[1]).toBe("eula.txt");
    await waitFor(() => expect(mocks.startServer).toHaveBeenCalledTimes(2));
  });
});
