// Shell rendering: sidebar reflects the servers store, badge reflects the
// connection store. The wire module is mocked out — no sockets in tests.

import { render, screen, cleanup } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { useConnection } from "../state/connection";
import { useServers } from "../state/servers";

vi.mock("../state/wire", () => ({
  client: {},
  startWire: vi.fn(),
}));

describe("App shell", () => {
  beforeEach(() => {
    useServers.setState({ servers: {}, crashes: {} });
    useConnection.setState({ status: "offline", daemon: null, lastError: null });
  });

  afterEach(cleanup);

  it("shows the daemon version and servers when online", () => {
    useConnection.setState({
      status: "ready",
      daemon: { name: "zamind", version: "9.9.9" },
    });
    useServers.setState({
      servers: {
        alpha: { serverId: "alpha", displayName: "Alpha", state: "running" },
        beta: { serverId: "beta", displayName: "Beta", state: "crashed" },
      },
      crashes: {},
    });

    render(<App />);

    expect(screen.getByText("zamind v9.9.9")).toBeTruthy();
    expect(screen.getByText("daemon online")).toBeTruthy();
    expect(screen.getByText("Alpha")).toBeTruthy();
    expect(screen.getByText("Beta")).toBeTruthy();
    expect(screen.getByLabelText("state: running")).toBeTruthy();
    expect(screen.getByLabelText("state: crashed")).toBeTruthy();
  });

  it("shows the offline badge when the daemon is unreachable", () => {
    useConnection.setState({ status: "offline", lastError: "connection refused" });
    render(<App />);
    expect(screen.getByText("daemon offline")).toBeTruthy();
    expect(screen.getByTitle("connection refused")).toBeTruthy();
  });

  it("nudges toward registration when the registry is empty", () => {
    useConnection.setState({ status: "ready", daemon: { name: "zamind", version: "1" } });
    render(<App />);
    expect(screen.getByText(/No servers registered yet/)).toBeTruthy();
  });
});
