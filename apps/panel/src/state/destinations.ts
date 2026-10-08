// Destinations (ADR-0015): the browser model's typed core. A tab shows a
// destination; an address names one. The founder's rule (§61) is identity
// discipline — a tab, a server, a process, a directory, and an address are
// not interchangeable — so destinations are a closed type, never strings.
//
// The address bar understands three dialects, in the founder's own order:
//   • internal ZaminPanel URLs (zaminpanel://servers/)
//   • server join addresses (localhost:25565, 0:25565, box.example.com:25565)
//   • free text, which is a discovery query, not a URL
// The Dutchmen conversation address (dutchmen:(id)) has its room reserved
// here as a documented dialect the parser will grow, not a live feature.

import type { ServerEntry } from "./servers";

export interface ServersDestination {
  kind: "servers";
}
export interface NewTabDestination {
  kind: "new";
}
export interface ServerDestination {
  kind: "server";
  serverId: string;
}
/** §27's dedicated console tab: one per server (§61 identity), the console
 *  optimized for large output — full height, same engine, its own tab. */
export interface ConsoleDestination {
  kind: "console";
  serverId: string;
}
export interface SettingsDestination {
  kind: "settings";
}
/** A typed request for an internal page that does not exist. The shell
 *  answers it honestly (§58: an unknown internal page is a real
 *  destination request, never silently a web search) — an error page,
 *  as a tab, because the operator typed it and deserves the history
 *  entry back out of it. */
export interface MissingPageDestination {
  kind: "missing";
  url: string;
}
/** The closed set of places a tab can show (§58: typed, never web pages). */
export type Destination =
  | ServersDestination
  | NewTabDestination
  | ServerDestination
  | ConsoleDestination
  | SettingsDestination
  | MissingPageDestination;

/** A tab's stable identity. Derived from the destination on purpose: one
 *  tab per server, one fleet page, one new-tab page (§6). */
export type TabKey = string;

export const SERVERS_TAB: TabKey = "servers";
export const NEW_TAB: TabKey = "new";
export const SETTINGS_TAB: TabKey = "settings";
export const serverTab = (serverId: string): TabKey => `server:${serverId}`;
export const consoleTab = (serverId: string): TabKey => `console:${serverId}`;

export function tabKey(destination: Destination): TabKey {
  switch (destination.kind) {
    case "servers":
      return SERVERS_TAB;
    case "new":
      return NEW_TAB;
    case "settings":
      return SETTINGS_TAB;
    case "server":
      return serverTab(destination.serverId);
    case "console":
      return consoleTab(destination.serverId);
    case "missing":
      return `missing:${destination.url}`;
  }
}

/** The canonical internal URL a destination is addressed by (§58). */
export function destinationUrl(destination: Destination): string {
  switch (destination.kind) {
    case "servers":
      return "zaminpanel://servers/";
    case "new":
      return "zaminpanel://new";
    case "settings":
      return "zaminpanel://settings/";
    case "server":
      return `zaminpanel://server/${destination.serverId}`;
    case "console":
      return `zaminpanel://console/${destination.serverId}`;
    case "missing":
      return destination.url;
  }
}

/** What the address bar shows when the tab rests. A server shows its join
 *  address when the port is known (§7) and its internal URL when it is
 *  not — an address the server cannot answer is not shown as if it could.
 *  The new tab rests empty (§6.2). */
export function restingAddress(
  destination: Destination,
  entries: ServerEntry[],
  host: string,
): string {
  if (destination.kind === "servers") return destinationUrl(destination);
  if (destination.kind === "new") return "";
  if (destination.kind === "settings" || destination.kind === "missing") {
    return destinationUrl(destination);
  }
  if (destination.kind === "console") {
    // The console tab rests at its internal URL: it is a ZaminPanel page,
    // not the server's join address (that stays the server tab's rest).
    return destinationUrl(destination);
  }
  const entry = entries.find((e) => e.serverId === destination.serverId);
  if (entry?.port) return joinAddress(entry, host);
  return destinationUrl(destination);
}

/** The join address of a managed server. The daemon owns the port; the
 *  host is this window's honest reach: localhost for the local daemon,
 *  the box's host for a remote profile (the daemon's bind address is its
 *  own business — 0.0.0.0 and localhost reach the same local server). */
export function joinAddress(entry: ServerEntry, host: string): string {
  return `${host}:${entry.port}`;
}

/** Where this window reaches a managed server's host part, from the
 *  active connection: local → localhost; remote → the agent's host. */
export function hostHint(connection: { local: boolean; remoteAddr?: string }): string {
  if (connection.local) return "localhost";
  const addr = connection.remoteAddr ?? "";
  // Split the trailing :port honestly (greedy `[^\]]+` would eat it);
  // bracketed IPv6 keeps its colons inside the brackets.
  const match = /^(\[[^\]]+\]|[^:]+)(?::\d+)?$/.exec(addr);
  const host = (match?.[1] ?? addr).replace(/^\[|\]$/g, "");
  return host || addr;
}

// --- the address bar's dialects (§7) -----------------------------------------

export type AddressRequest =
  | { kind: "internal"; destination: Destination }
  | { kind: "join"; host?: string; port: number }
  | { kind: "query"; text: string };

const INTERNAL_URL = /^zaminpanel:\/\/([^/?#]+)\/?(?:([^/?#]+))?$/;
/** `host:port`, `:port`, or a bare port — the join dialect. A host may be
 *  an IPv4, a hostname, or a bracketed IPv6; the port is 1-65535. */
const JOIN = /^(?:\[?([a-zA-Z0-9._-]+|\[[0-9a-fA-F:]+\])\]?)?:(\d{1,5})$|^\[?([a-zA-Z0-9._-]+)\]?$/;

/** Parse what the operator typed, before any store is consulted. */
export function parseAddressInput(text: string): AddressRequest {
  const trimmed = text.trim();
  const internal = INTERNAL_URL.exec(trimmed);
  if (internal) {
    const page = internal[1];
    if (page === "servers") return { kind: "internal", destination: { kind: "servers" } };
    if (page === "new") return { kind: "internal", destination: { kind: "new" } };
    if (page === "settings") return { kind: "internal", destination: { kind: "settings" } };
    if (page === "server" && internal[2]) {
      return { kind: "internal", destination: { kind: "server", serverId: internal[2] } };
    }
    if (page === "console" && internal[2]) {
      return { kind: "internal", destination: { kind: "console", serverId: internal[2] } };
    }
    // An unknown internal page is a real destination request the shell
    // answers honestly (no such page) — not silently a web search.
    return { kind: "internal", destination: { kind: "missing", url: trimmed } };
  }
  const join = JOIN.exec(trimmed);
  if (join) {
    if (join[2]) {
      const port = Number(join[2]);
      if (port >= 1 && port <= 65_535) {
        return { kind: "join", host: normalizeHost(join[1]), port };
      }
    } else if (join[3]) {
      // A bare word could be a port ("25565") — only digits qualify.
      if (/^\d{1,5}$/.test(join[3])) {
        const port = Number(join[3]);
        if (port >= 1 && port <= 65_535) return { kind: "join", port };
      }
    }
  }
  return { kind: "query", text: trimmed };
}

function normalizeHost(host: string | undefined): string | undefined {
  if (!host) return undefined;
  const unbracketed = host.replace(/^\[|\]$/g, "");
  // The bind-all and loopback spellings reach the same local server —
  // including the founder's own "0:25565" shorthand for bind-all.
  if (
    unbracketed === "0" ||
    unbracketed === "0.0.0.0" ||
    unbracketed === "127.0.0.1" ||
    unbracketed === "::"
  ) {
    return "localhost";
  }
  return unbracketed;
}

/** Resolve a join request against the registry: the port is the match;
 *  a typed host only agrees or disagrees with this window's own hint. */
export function resolveJoin(
  request: { host?: string; port: number },
  entries: ServerEntry[],
  host: string,
): ServerEntry | null {
  const matches = entries.filter((e) => e.port === request.port);
  if (matches.length === 0) return null;
  if (matches.length === 1) return matches[0] ?? null;
  // Several servers share the port (possible across boxes in future
  // remote fleets): prefer an exact host match, else say nothing yet.
  const exact = matches.find((e) => joinAddress(e, host) === `${request.host ?? host}:${request.port}`);
  return exact ?? null;
}

/** Discovery (§22, deterministic form): the registry answers the query
 *  with its rows — name or id containing the text, case-insensitive. */
export function searchServers(text: string, entries: ServerEntry[]): ServerEntry[] {
  const needle = text.trim().toLowerCase();
  if (needle === "") return entries;
  return entries.filter(
    (e) =>
      e.displayName.toLowerCase().includes(needle) ||
      e.serverId.toLowerCase().includes(needle),
  );
}
