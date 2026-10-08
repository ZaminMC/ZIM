// The destination model's contract: tabs and addresses are typed, the
// address bar's three dialects parse without a store, join resolution
// matches on the port with honest host agreement, and discovery is a
// substring query over the registry. Identity discipline (§61) is the
// point — nothing here lets a string become a server.

import { describe, expect, it } from "vitest";
import {
  NEW_TAB,
  SERVERS_TAB,
  consoleTab,
  destinationUrl,
  hostHint,
  joinAddress,
  parseAddressInput,
  resolveJoin,
  restingAddress,
  searchServers,
  serverTab,
  tabKey,
  type Destination,
} from "./destinations";
import type { ServerEntry } from "./servers";

const entry = (id: string, extra: Partial<ServerEntry> = {}): ServerEntry => ({
  serverId: id,
  displayName: id,
  state: "stopped",
  ...extra,
});

describe("tab keys and URLs", () => {
  it("derives one stable key per destination kind", () => {
    expect(tabKey({ kind: "servers" })).toBe(SERVERS_TAB);
    expect(tabKey({ kind: "new" })).toBe(NEW_TAB);
    expect(tabKey({ kind: "server", serverId: "alpha" })).toBe("server:alpha");
    expect(serverTab("alpha")).toBe("server:alpha");
  });

  it("names every destination with an internal URL (§58)", () => {
    const destinations: Destination[] = [
      { kind: "servers" },
      { kind: "new" },
      { kind: "settings" },
      { kind: "server", serverId: "alpha" },
    ];
    expect(destinations.map(destinationUrl)).toEqual([
      "zaminpanel://servers/",
      "zaminpanel://new",
      "zaminpanel://settings/",
      "zaminpanel://server/alpha",
    ]);
  });
});

describe("restingAddress", () => {
  const entries = [entry("alpha", { displayName: "Alpha", port: 25565 })];

  it("shows the join address when the port is known (§7)", () => {
    expect(restingAddress({ kind: "server", serverId: "alpha" }, entries, "localhost")).toBe(
      "localhost:25565",
    );
  });

  it("falls back to the internal URL when the server never booted", () => {
    const unbooted = [entry("beta", { displayName: "Beta" })];
    expect(restingAddress({ kind: "server", serverId: "beta" }, unbooted, "localhost")).toBe(
      "zaminpanel://server/beta",
    );
  });

  it("shows the internal URL for the fleet page and rests empty on new (§6.2)", () => {
    expect(restingAddress({ kind: "servers" }, entries, "localhost")).toBe(
      "zaminpanel://servers/",
    );
    expect(restingAddress({ kind: "new" }, entries, "localhost")).toBe("");
  });
});

describe("hostHint", () => {
  it("says localhost for the local daemon and the box's host for remote", () => {
    expect(hostHint({ local: true })).toBe("localhost");
    expect(hostHint({ local: false, remoteAddr: "192.168.1.50:7443" })).toBe("192.168.1.50");
  });
});

describe("parseAddressInput", () => {
  it("parses internal zaminpanel:// URLs into destinations", () => {
    expect(parseAddressInput("zaminpanel://servers/")).toEqual({
      kind: "internal",
      destination: { kind: "servers" },
    });
    expect(parseAddressInput("zaminpanel://new")).toEqual({
      kind: "internal",
      destination: { kind: "new" },
    });
    expect(parseAddressInput("zaminpanel://server/alpha")).toEqual({
      kind: "internal",
      destination: { kind: "server", serverId: "alpha" },
    });
    expect(parseAddressInput("zaminpanel://settings/")).toEqual({
      kind: "internal",
      destination: { kind: "settings" },
    });
  });

  it("answers an unknown internal page as a typed missing destination, not a search", () => {
    expect(parseAddressInput("zaminpanel://nope/")).toEqual({
      kind: "internal",
      destination: { kind: "missing", url: "zaminpanel://nope/" },
    });
  });

  it("parses the join dialect, normalizing bind-all and loopback", () => {
    expect(parseAddressInput("localhost:25565")).toEqual({
      kind: "join",
      host: "localhost",
      port: 25565,
    });
    expect(parseAddressInput("0:25565")).toEqual({ kind: "join", host: "localhost", port: 25565 });
    expect(parseAddressInput("0.0.0.0:25566")).toEqual({
      kind: "join",
      host: "localhost",
      port: 25566,
    });
    expect(parseAddressInput("192.168.1.50:25565")).toEqual({
      kind: "join",
      host: "192.168.1.50",
      port: 25565,
    });
    expect(parseAddressInput(":25565")).toEqual({ kind: "join", port: 25565 });
    expect(parseAddressInput("25565")).toEqual({ kind: "join", port: 25565 });
  });

  it("refuses impossible ports honestly — they are queries, not joins", () => {
    expect(parseAddressInput("99999")).toEqual({ kind: "query", text: "99999" });
    expect(parseAddressInput("localhost:99999")).toEqual({
      kind: "query",
      text: "localhost:99999",
    });
  });

  it("treats free text as a discovery query, never a URL", () => {
    expect(parseAddressInput("What are the active servers?")).toEqual({
      kind: "query",
      text: "What are the active servers?",
    });
    expect(parseAddressInput("  alpha  ")).toEqual({ kind: "query", text: "alpha" });
  });
});

describe("resolveJoin", () => {
  const entries = [
    entry("alpha", { displayName: "Alpha", port: 25565 }),
    entry("beta", { displayName: "Beta", port: 25566 }),
  ];

  it("matches on the port alone", () => {
    expect(resolveJoin({ port: 25566 }, entries, "localhost")?.serverId).toBe("beta");
  });

  it("answers null when nothing is registered on the port", () => {
    expect(resolveJoin({ port: 25577 }, entries, "localhost")).toBeNull();
  });

  it("prefers the exact host when several servers share a port", () => {
    const remote = entry("boxy", { displayName: "Boxy", port: 25565 });
    const both = [...entries, remote];
    // The panel manages local servers today; two same-port rows only
    // resolve when the typed host agrees with one of them.
    expect(resolveJoin({ host: "localhost", port: 25565 }, both, "localhost")?.serverId).toBe(
      "alpha",
    );
    expect(resolveJoin({ host: "elsewhere", port: 25565 }, both, "localhost")).toBeNull();
  });
});

describe("searchServers", () => {
  const entries = [
    entry("alpha", { displayName: "Alpha" }),
    entry("beta", { displayName: "Beta" }),
  ];

  it("filters by display name or id, case-insensitively", () => {
    expect(searchServers("alp", entries).map((e) => e.serverId)).toEqual(["alpha"]);
    expect(searchServers("BETA", entries).map((e) => e.serverId)).toEqual(["beta"]);
  });

  it("returns the whole registry for an empty query (§22)", () => {
    expect(searchServers("", entries)).toHaveLength(2);
  });
});

describe("joinAddress", () => {
  it("joins the window's reach with the daemon's port", () => {
    expect(joinAddress(entry("alpha", { port: 25565 }), "localhost")).toBe("localhost:25565");
    expect(joinAddress(entry("alpha", { port: 25565 }), "box.example.com")).toBe(
      "box.example.com:25565",
    );
  });
});

describe("console destinations (§27, ADR-0020)", () => {
  it("derives one console tab per server", () => {
    expect(tabKey({ kind: "console", serverId: "alpha" })).toBe("console:alpha");
    expect(consoleTab("alpha")).toBe("console:alpha");
  });

  it("names the console tab with its internal URL", () => {
    expect(destinationUrl({ kind: "console", serverId: "alpha" })).toBe(
      "zaminpanel://console/alpha",
    );
    expect(restingAddress({ kind: "console", serverId: "alpha" }, [], "localhost")).toBe(
      "zaminpanel://console/alpha",
    );
  });

  it("parses the console URL from the address bar", () => {
    expect(parseAddressInput("zaminpanel://console/alpha")).toEqual({
      kind: "internal",
      destination: { kind: "console", serverId: "alpha" },
    });
  });

  it("answers an id-less console URL with the honest missing page", () => {
    const request = parseAddressInput("zaminpanel://console");
    expect(request).toEqual({
      kind: "internal",
      destination: { kind: "missing", url: "zaminpanel://console" },
    });
  });
});
