// The fleet page (zim://servers/, ADR-0015): the registry as cards over
// the authoritative store. Pinned: the subtitle's three honest states
// (waiting / online without identity / online with the daemon's version
// and count), the stat triple, the instant filter that appears only once
// the fleet is big enough to need it and matches both the Server ID and
// the alias, the cards' keyboard-and-pointer openness, and §53's empty
// state keeping its documented nudge.

import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FleetPage } from "./FleetPage";
import { useConnection } from "../state/connection";
import type { ServerEntry } from "../state/servers";

function entryOf(over: Partial<ServerEntry>): ServerEntry {
  return {
    serverId: "alpha",
    displayName: "Alpha",
    state: "stopped",
    ...over,
  };
}

const FOUR: ServerEntry[] = [
  entryOf({ serverId: "alpha", displayName: "Alpha", state: "running" }),
  entryOf({ serverId: "beta", displayName: "Beta Events", state: "starting" }),
  entryOf({ serverId: "gamma", displayName: "Gamma", state: "stopping" }),
  entryOf({ serverId: "delta", displayName: "Delta", state: "crashed" }),
];

beforeEach(() => {
  useConnection.setState({ status: "ready", daemon: { name: "zamind", version: "0.4.34" } });
});

afterEach(cleanup);

describe("the subtitle", () => {
  it("waits honestly while the connection is not ready", () => {
    useConnection.setState({ status: "connecting", daemon: null });
    render(<FleetPage servers={[]} onOpen={() => {}} onNewServer={() => {}} />);
    expect(screen.getByText("Waiting for ZIM…")).toBeTruthy();
  });

  it("online without the daemon's identity says just that", () => {
    useConnection.setState({ status: "ready", daemon: null });
    render(<FleetPage servers={[]} onOpen={() => {}} onNewServer={() => {}} />);
    expect(screen.getByText("ZIM is online")).toBeTruthy();
  });

  it("online with the daemon names the version and the running count", () => {
    render(
      <FleetPage servers={FOUR} onOpen={() => {}} onNewServer={() => {}} />,
    );
    expect(screen.getByText("ZIM v0.4.34 is online — 1 of 4 running")).toBeTruthy();
  });
});

describe("the stats and the grid", () => {
  it("counts servers, running, and in-transition honestly", () => {
    render(<FleetPage servers={FOUR} onOpen={() => {}} onNewServer={() => {}} />);
    const stats = screen.getAllByText(/^(4|1|2)$/);
    expect(stats.map((s) => s.textContent).sort()).toEqual(["1", "2", "4"]);
    // "Servers" is the page title AND a stat label; "Running" is also a
    // status chip's word — both may match more than once.
    expect(screen.getAllByText("Servers").length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByText("Running").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("In transition")).toBeTruthy();
  });

  it("renders one card per server with its meta chips", () => {
    const servers = [
      entryOf({ serverId: "alpha", displayName: "Alpha", software: "paper", version: "1.21", port: 25565 }),
    ];
    render(<FleetPage servers={servers} onOpen={() => {}} onNewServer={() => {}} />);
    expect(screen.getByText("Alpha")).toBeTruthy();
    expect(screen.getByText("alpha")).toBeTruthy();
    expect(screen.getByText("paper")).toBeTruthy();
    expect(screen.getByText("v1.21")).toBeTruthy();
    expect(screen.getByText(":25565")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open Alpha" })).toBeTruthy();
  });

  it("a card opens by click, Enter, and Space — pointer and keyboard are equals", () => {
    const onOpen = vi.fn();
    render(<FleetPage servers={FOUR} onOpen={onOpen} onNewServer={() => {}} />);
    const card = screen.getByRole("button", { name: "Open Delta" });
    fireEvent.click(card);
    expect(onOpen).toHaveBeenLastCalledWith("delta");
    fireEvent.keyDown(card, { key: "Enter" });
    expect(onOpen).toHaveBeenLastCalledWith("delta");
    fireEvent.keyDown(card, { key: " " });
    expect(onOpen).toHaveBeenLastCalledWith("delta");
    expect(onOpen).toHaveBeenCalledTimes(3);
  });
});

describe("the filter", () => {
  function bigFleet(): ServerEntry[] {
    return [
      ...FOUR,
      entryOf({ serverId: "echo", displayName: "Echo" }),
      entryOf({ serverId: "foxtrot", displayName: "Foxtrot" }),
    ];
  }

  it("exists only once the fleet is bigger than three — three cards need no filter", () => {
    const three = FOUR.slice(0, 3);
    const { rerender } = render(
      <FleetPage servers={three} onOpen={() => {}} onNewServer={() => {}} />,
    );
    expect(screen.queryByRole("textbox", { name: "Filter servers by ID or name" })).toBeNull();
    rerender(<FleetPage servers={FOUR} onOpen={() => {}} onNewServer={() => {}} />);
    expect(
      screen.getByRole("textbox", { name: "Filter servers by ID or name" }),
    ).toBeTruthy();
  });

  it("matches the Server ID and the display name, case-insensitively, per keystroke", () => {
    render(<FleetPage servers={bigFleet()} onOpen={() => {}} onNewServer={() => {}} />);
    const filter = screen.getByRole("textbox", { name: "Filter servers by ID or name" });
    fireEvent.change(filter, { target: { value: "EVE" } });
    expect(screen.getByRole("button", { name: "Open Beta Events" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Open Alpha" })).toBeNull();
    fireEvent.change(filter, { target: { value: "FOXTROT" } });
    expect(screen.getByRole("button", { name: "Open Foxtrot" })).toBeTruthy();
  });

  it("a miss is a stated empty result, and the clear button restores the fleet", () => {
    render(<FleetPage servers={bigFleet()} onOpen={() => {}} onNewServer={() => {}} />);
    const filter = screen.getByRole("textbox", { name: "Filter servers by ID or name" });
    fireEvent.change(filter, { target: { value: "nope" } });
    expect(screen.getByText(/No server matches “nope”/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Clear filter" }));
    expect((filter as HTMLInputElement).value).toBe("");
    expect(screen.getByRole("button", { name: "Open Alpha" })).toBeTruthy();
  });
});

describe("the empty state", () => {
  it("keeps §53's nudge and wires the New server affordance", () => {
    const onNewServer = vi.fn();
    render(<FleetPage servers={[]} onOpen={() => {}} onNewServer={onNewServer} />);
    expect(screen.getByText("No servers yet.")).toBeTruthy();
    expect(screen.getByText(/to register an existing server directory/)).toBeTruthy();
    const buttons = screen.getAllByRole("button", { name: /New server/ });
    fireEvent.click(buttons[0]!);
    expect(onNewServer).toHaveBeenCalledTimes(1);
  });

  it("the header's New server is always offered", () => {
    const onNewServer = vi.fn();
    render(<FleetPage servers={FOUR} onOpen={() => {}} onNewServer={onNewServer} />);
    fireEvent.click(screen.getAllByRole("button", { name: /New server/ })[0]!);
    expect(onNewServer).toHaveBeenCalledTimes(1);
  });
});
