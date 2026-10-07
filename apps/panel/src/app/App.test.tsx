// The browser shell (ADR-0015): tabs, the address bar's dialects, the new
// tab's discovery, and the chrome's honest states. The wire module is
// mocked out — no sockets in tests. Server workspace internals are
// covered by ServerView.test; here the frame is the subject.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { useConnection } from "../state/connection";
import { useConnections } from "../state/connections";
import { useServers } from "../state/servers";
import { tabKeyOf, useTabs } from "../state/tabs";
import { getServer } from "../state/actions";

vi.mock("../state/wire", () => ({
  client: {},
  startWire: vi.fn(),
}));

vi.mock("../state/actions", () => ({
  getServer: vi.fn(),
  startServer: vi.fn(),
  stopServer: vi.fn(),
  restartServer: vi.fn(),
  killServer: vi.fn(),
  writeWholeFile: vi.fn(),
  metricsRange: vi.fn(async () => ({ samples: [] })),
  subscribeMetrics: vi.fn(async () => ({ dispose: () => {} })),
}));

// The console is the heaviest surface (xterm) and irrelevant to the frame.
vi.mock("./Console", () => ({ Console: () => null }));

const seedTabs = () =>
  useTabs.setState({
    tabs: [{ history: [{ kind: "new" }], historyIndex: 0, reloadToken: 0 }],
    activeKey: "new",
    discoveryQuery: null,
  });

beforeEach(() => {
  seedTabs();
  useServers.setState({ servers: {}, crashes: {} });
  useConnection.setState({ status: "ready", daemon: { name: "zamind", version: "1.2.3" }, lastError: null });
  useConnections.setState({ remotes: [], activeId: "local" });
  vi.mocked(getServer).mockReset();
  vi.mocked(getServer).mockResolvedValue({
    serverId: "alpha",
    displayName: "Alpha",
    state: "running",
    port: 25565,
  });
});

afterEach(cleanup);

const addressInput = () => screen.getByLabelText<HTMLInputElement>("Address bar");

const typeAddress = (text: string) => {
  const input = addressInput();
  fireEvent.focus(input);
  fireEvent.change(input, { target: { value: text } });
  fireEvent.keyDown(input, { key: "Enter" });
};

describe("the shell at boot", () => {
  it("opens on the new tab, honest about an empty registry", () => {
    render(<App />);
    expect(screen.getByText("Find a server")).toBeTruthy();
    expect(screen.getByText(/No servers registered yet/)).toBeTruthy();
    expect(useTabs.getState().activeKey).toBe("new");
  });

  it("carries the daemon pulse and version in the chrome", () => {
    render(<App />);
    expect(screen.getByLabelText("daemon online")).toBeTruthy();
    const menu = screen.getByRole("button", { name: "Menu" });
    fireEvent.click(menu);
    expect(screen.getByText("zamind v1.2.3")).toBeTruthy();
  });
});

describe("the address bar's dialects", () => {
  it("navigates internal URLs to the fleet page", () => {
    useServers.setState({
      servers: {
        alpha: { serverId: "alpha", displayName: "Alpha", state: "running", port: 25565 },
      },
      crashes: {},
    });
    render(<App />);
    typeAddress("zaminpanel://servers/");
    expect(screen.getByRole("heading", { name: "Servers" })).toBeTruthy();
    expect(screen.getByText("Alpha")).toBeTruthy();
    expect(useTabs.getState().activeKey).toBe("servers");
  });

  it("answers an unknown internal page with an honest page, and backs out of it", () => {
    render(<App />);
    typeAddress("zaminpanel://nope/");
    expect(screen.getByText("No such page")).toBeTruthy();
    expect(useTabs.getState().activeKey).toBe("missing:zaminpanel://nope/");

    fireEvent.click(screen.getByRole("button", { name: "Back (Alt+Left)" }));
    expect(useTabs.getState().activeKey).toBe("new");
    expect(screen.getByText("Find a server")).toBeTruthy();
  });

  it("opens a server by its join address", async () => {
    useServers.setState({
      servers: {
        alpha: { serverId: "alpha", displayName: "Alpha", state: "running", port: 25565 },
      },
      crashes: {},
    });
    render(<App />);
    typeAddress("localhost:25565");
    await waitFor(() => expect(useTabs.getState().activeKey).toBe("server:alpha"));
    // The workspace opened: the tab strip wears the server's name (the
    // workspace's own surfaces render role=tab too, so scan the selected
    // ones), and the address bar rests on the join address.
    const selectedTabs = screen.getAllByRole("tab", { selected: true });
    expect(selectedTabs.some((t) => t.textContent?.includes("Alpha"))).toBe(true);
    expect(addressInput().value).toBe("localhost:25565");
  });

  it("refuses a join address nothing is registered on — no navigation, a note instead", () => {
    render(<App />);
    typeAddress(":25577");
    expect(useTabs.getState().activeKey).toBe("new");
    expect(screen.getByRole("status").textContent).toContain("Nothing registered on :25577");
  });

  it("treats free text as discovery and filters the new tab's list", () => {
    useServers.setState({
      servers: {
        alpha: { serverId: "alpha", displayName: "Alpha", state: "running", port: 25565 },
        beta: { serverId: "beta", displayName: "Beta", state: "stopped" },
      },
      crashes: {},
    });
    render(<App />);
    const input = screen.getByLabelText<HTMLInputElement>("Search servers or type an address");
    fireEvent.change(input, { target: { value: "alp" } });
    expect(screen.getByText("Alpha")).toBeTruthy();
    expect(screen.queryByText("Beta")).toBeNull();
  });
});

describe("tabs behave like tabs", () => {
  it("keeps one tab per server: opening an open server focuses it", () => {
    useServers.setState({
      servers: {
        alpha: { serverId: "alpha", displayName: "Alpha", state: "running", port: 25565 },
      },
      crashes: {},
    });
    useTabs.setState({
      tabs: [
        { history: [{ kind: "new" }], historyIndex: 0, reloadToken: 0 },
        { history: [{ kind: "server", serverId: "alpha" }], historyIndex: 0, reloadToken: 0 },
      ],
      activeKey: "new",
    });
    render(<App />);
    typeAddress("zaminpanel://server/alpha");
    const s = useTabs.getState();
    expect(s.tabs).toHaveLength(2);
    expect(s.activeKey).toBe("server:alpha");
  });

  it("back and forward ride the active tab's destination history", () => {
    render(<App />);
    typeAddress("zaminpanel://servers/");
    expect(useTabs.getState().activeKey).toBe("servers");

    const back = screen.getByRole<HTMLButtonElement>("button", { name: "Back (Alt+Left)" });
    const forward = screen.getByRole<HTMLButtonElement>("button", { name: "Forward (Alt+Right)" });
    expect(back.disabled).toBe(false);
    expect(forward.disabled).toBe(true);

    fireEvent.click(back);
    expect(useTabs.getState().activeKey).toBe("new");
    expect(forward.disabled).toBe(false);

    fireEvent.click(forward);
    expect(useTabs.getState().activeKey).toBe("servers");
  });

  it("closing the active tab hands over to a neighbor, and the last close leaves a new tab", () => {
    useTabs.setState({
      tabs: [
        { history: [{ kind: "servers" }], historyIndex: 0, reloadToken: 0 },
        { history: [{ kind: "settings" }], historyIndex: 0, reloadToken: 0 },
      ],
      activeKey: "settings",
    });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Close Settings" }));
    expect(useTabs.getState().activeKey).toBe("servers");

    fireEvent.click(screen.getByRole("button", { name: "Close Servers" }));
    expect(useTabs.getState().activeKey).toBe("new");
    expect(screen.getByText("Find a server")).toBeTruthy();
  });

  it("Ctrl+T opens the new tab and Ctrl+W closes the active one", () => {
    render(<App />);
    fireEvent.keyDown(window, { key: "t", ctrlKey: true });
    typeAddress("zaminpanel://servers/");
    expect(useTabs.getState().activeKey).toBe("servers");

    fireEvent.keyDown(window, { key: "t", ctrlKey: true });
    expect(useTabs.getState().activeKey).toBe("new");

    fireEvent.keyDown(window, { key: "w", ctrlKey: true });
    expect(useTabs.getState().activeKey).toBe("servers");
  });

  it("reload bumps only the active tab's token", () => {
    useTabs.setState({
      tabs: [
        { history: [{ kind: "servers" }], historyIndex: 0, reloadToken: 1 },
        { history: [{ kind: "settings" }], historyIndex: 0, reloadToken: 0 },
      ],
      activeKey: "servers",
    });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Reload (Ctrl+R)" }));
    const s = useTabs.getState();
    expect(s.tabs.find((t) => tabKeyOf(t) === "servers")!.reloadToken).toBe(2);
    expect(s.tabs.find((t) => tabKeyOf(t) === "settings")!.reloadToken).toBe(0);
  });
});
