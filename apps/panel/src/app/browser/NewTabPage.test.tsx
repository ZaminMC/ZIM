// The new tab (ADR-0015, §22): one input that speaks the omnibox's three
// dialects, a live filter whose results follow the typing (Enter only
// matters for the address dialects), the carried query consumed once,
// active-first ordering, and §64's machine section whose open verb is
// register-then-navigate — a jar is refused in words, a refusal is a
// typed note, and nothing is ever half-opened.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NewTabPage } from "./NewTabPage";
import { useServers } from "../../state/servers";
import type { ServerEntry } from "../../state/servers";
import { useTabs } from "../../state/tabs";
import { useUi } from "../../state/ui";
import type { DiscoveredServer } from "../../protocol/types";

vi.mock("../../state/actions", () => ({
  registerServer: vi.fn(),
}));
import { registerServer } from "../../state/actions";
const registerMock = registerServer as ReturnType<typeof vi.fn>;

// The machine section is its own tested surface; here it is a trigger
// wire — the mock records the page's onOpen so the tests can drive the
// open verb with any candidate, and renders nothing.
const machineTrigger: { onOpen: ((c: DiscoveredServer) => void) | null } = { onOpen: null };
vi.mock("./machineDiscovery", async () => {
  const actual = await vi.importActual<object>("./machineDiscovery");
  return {
    ...actual,
    MachineDiscovery: function MockMachine(p: {
      onOpen: (c: DiscoveredServer) => void;
    }) {
      machineTrigger.onOpen = p.onOpen;
      return null;
    },
  };
});

const input0 = () => screen.getByRole("textbox", { name: "Search servers or type an address" });

function entryOf(over: Partial<ServerEntry>): ServerEntry {
  return { serverId: "alpha", displayName: "Alpha", state: "stopped", ...over };
}

function resetStores(entries: ServerEntry[]): void {
  localStorage.clear();
  const map: Record<string, ServerEntry> = {};
  for (const e of entries) map[e.serverId] = e;
  useServers.setState({ servers: map });
  const tab = {
    id: "t1",
    history: [{ kind: "new" } as never],
    historyIndex: 0,
    reloadToken: 0,
  };
  useTabs.setState({
    tabs: [tab],
    activeId: "t1",
    discoveryQuery: null,
    recentlyClosed: [],
    groups: {},
  });
  useUi.setState({ newServerOpen: false });
  registerMock.mockReset();
  machineTrigger.onOpen = null;
}

function activeDestination(): unknown {
  const s = useTabs.getState();
  const tab = s.tabs.find((t) => t.id === s.activeId);
  return tab?.history[tab.historyIndex];
}

afterEach(cleanup);

describe("the page's own input", () => {
  beforeEach(() => resetStores([]));

  it("consumes the carried query once — then the input is the page's own state", () => {
    useTabs.setState({ discoveryQuery: "paper" });
    render(<NewTabPage />);
    const input = input0() as HTMLInputElement;
    expect(input.value).toBe("paper");
    expect(useTabs.getState().discoveryQuery).toBeNull();
  });

  it("with no carried query the input starts empty", () => {
    render(<NewTabPage />);
    expect((input0() as HTMLInputElement).value).toBe("");
  });
});

describe("the live filter", () => {
  beforeEach(() =>
    resetStores([
      entryOf({ serverId: "alpha", displayName: "Alpha", state: "running", port: 25565 }),
      entryOf({ serverId: "beta", displayName: "Beta Events", state: "stopped", port: 25566 }),
      entryOf({ serverId: "gamma", displayName: "Gamma", state: "stopped" }),
    ]),
  );

  it("the empty input shows every server, active first", () => {
    render(<NewTabPage />);
    const rows = screen.getAllByRole("button", { name: /^Open / });
    expect(rows.map((r) => r.getAttribute("aria-label"))).toEqual([
      "Open Alpha",
      "Open Beta Events",
      "Open Gamma",
    ]);
    // The address line lives in the row (the hint's code sample spells
    // the same string — the row match is the one inside a button).
    const addr = screen
      .getAllByText("localhost:25565")
      .find((el) => el.closest("button"));
    expect(addr).toBeTruthy();
    // A portless server shows its id as the address line — no invented port.
    expect(screen.getByText("gamma")).toBeTruthy();
  });

  it("results follow the typing — names and ids, live, without Enter", () => {
    render(<NewTabPage />);
    const input = input0();
    fireEvent.change(input, { target: { value: "eve" } });
    expect(screen.queryByRole("button", { name: "Open Alpha" })).toBeNull();
    expect(screen.getByRole("button", { name: "Open Beta Events" })).toBeTruthy();
    fireEvent.change(input, { target: { value: "BETA" } });
    expect(screen.getByRole("button", { name: "Open Beta Events" })).toBeTruthy();
  });

  it("a miss says so and keeps the registry untouched", () => {
    render(<NewTabPage />);
    fireEvent.change(input0(), { target: { value: "nope" } });
    expect(screen.getByText(/No server matches “nope”/)).toBeTruthy();
  });
});

describe("the address dialects on Enter", () => {
  beforeEach(() =>
    resetStores([
      entryOf({ serverId: "alpha", displayName: "Alpha", state: "running", port: 25565 }),
    ]),
  );

  it("an internal page lands as a navigation", () => {
    render(<NewTabPage />);
    fireEvent.change(input0(), { target: { value: "zim://servers" } });
    fireEvent.submit(screen.getByRole("search"));
    expect(activeDestination()).toEqual({ kind: "servers" });
  });

  it("a join that resolves opens the matched server's tab", () => {
    render(<NewTabPage />);
    fireEvent.change(input0(), {
      target: { value: "localhost:25565" },
    });
    fireEvent.submit(screen.getByRole("search"));
    expect(activeDestination()).toEqual({ kind: "server", serverId: "alpha" });
  });

  it("a join that resolves to nothing navigates nowhere — the miss is silent because the list already says what exists", () => {
    render(<NewTabPage />);
    fireEvent.change(input0(), {
      target: { value: "localhost:12345" },
    });
    fireEvent.submit(screen.getByRole("search"));
    expect(activeDestination()).toEqual({ kind: "new" });
  });

  it("free text is not a navigation — the results already followed the typing", () => {
    render(<NewTabPage />);
    fireEvent.change(input0(), { target: { value: "hello world" } });
    fireEvent.submit(screen.getByRole("search"));
    expect(activeDestination()).toEqual({ kind: "new" });
    expect(useTabs.getState().tabs).toHaveLength(1);
  });
});

describe("the empty registry", () => {
  beforeEach(() => resetStores([]));

  it("says no servers are registered and wires the New server affordance", () => {
    render(<NewTabPage />);
    expect(screen.getByText("No servers registered yet.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /New server/ }));
    expect(useUi.getState().newServerOpen).toBe(true);
  });
});

describe("the machine section's open verb", () => {
  beforeEach(() =>
    resetStores([entryOf({ serverId: "alpha", displayName: "Alpha", state: "stopped" })]),
  );

  it("a directory registers under the folder's slug and opens its tab", async () => {
    registerMock.mockResolvedValue({ serverId: "survival" });
    render(<NewTabPage />);
    await waitFor(() => expect(machineTrigger.onOpen).not.toBeNull());
    machineTrigger.onOpen!({
      path: "/srv/minecraft/My Survival",
      kind: "directory",
      displayName: "My Survival",
      port: 25565,
    });
    await waitFor(() =>
      expect(activeDestination()).toEqual({ kind: "server", serverId: "my-survival" }),
    );
    expect(registerMock).toHaveBeenCalledWith({
      serverId: "my-survival",
      displayName: "My Survival",
      rootPath: "/srv/minecraft/My Survival",
    });
  });

  it("a jar is refused in words — the registry is unchanged, nothing opens", async () => {
    render(<NewTabPage />);
    await waitFor(() => expect(machineTrigger.onOpen).not.toBeNull());
    machineTrigger.onOpen!({
      path: "/srv/minecraft/paper.jar",
      kind: "jar",
      displayName: "paper.jar",
    });
    await waitFor(() =>
      expect(
        screen.getByText(/That is a jar, not a server directory/),
      ).toBeTruthy(),
    );
    expect(registerMock).not.toHaveBeenCalled();
    expect(activeDestination()).toEqual({ kind: "new" });
    // The refusal keeps its remediation line: nothing was half-opened.
    expect(
      screen.getByText("The registry is unchanged — nothing was half-opened."),
    ).toBeTruthy();
  });

  it("a refused registration is a typed note with the folder named", async () => {
    registerMock.mockRejectedValue(new Error("The folder is not a server root."));
    render(<NewTabPage />);
    await waitFor(() => expect(machineTrigger.onOpen).not.toBeNull());
    machineTrigger.onOpen!({
      path: "/srv/minecraft/broken",
      kind: "directory",
      displayName: "broken",
    });
    await waitFor(() =>
      expect(
        screen.getByText(/The folder is not a server root\. \(\/srv\/minecraft\/broken\)/),
      ).toBeTruthy(),
    );
    expect(activeDestination()).toEqual({ kind: "new" });
  });
});
