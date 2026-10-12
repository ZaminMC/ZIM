// Destinations (ADR-0015): the browser model's typed core. A tab shows a
// destination; an address names one. The founder's rule (§61) is identity
// discipline — a tab, a server, a process, a directory, and an address are
// not interchangeable — so destinations are a closed type, never strings.
//
// The address bar understands three dialects, in the founder's own order:
//   • internal ZIM URLs (zim://servers/)
//   • server join addresses (localhost:25565, 0:25565, box.example.com:25565)
//   • free text, which is a discovery query, not a URL
// The Dutchmen conversation address (dutchmen:(id)) has its room reserved
// here as a documented dialect the parser will grow, not a live feature.

import type { ServerEntry } from "./servers";

/** The addressable pages of a server's dashboard (§8–§12's URL model):
 *  `host:port/<page>` is the visible address of each; the dashboard
 *  root (`host:port`) carries no page word. The set is the sidebar's own
 *  closed list — a URL naming anything else is an honest Missing, never
 *  a silent dashboard. */
export const SERVER_PAGES = [
  "console",
  "metrics",
  "logs",
  "files",
  "players",
  "plugins",
  "schedules",
  "network",
  "startup",
  "settings",
  "backups",
] as const;

export type ServerPage = (typeof SERVER_PAGES)[number];

export const isServerPage = (word: string): word is ServerPage =>
  (SERVER_PAGES as readonly string[]).includes(word);

export interface ServersDestination {
  kind: "servers";
}
export interface NewTabDestination {
  kind: "new";
}
export interface ServerDestination {
  kind: "server";
  serverId: string;
  /** The dashboard page this tab addresses (`host:port/<page>`);
   *  `undefined` = the dashboard root. Pages are history — one tab per
   *  server (§61), the page rides the history stack, not the identity. */
  page?: ServerPage;
  /** The files page's addressed path (`host:port/files/eula.txt`) —
   *  server-root-relative, `/`-joined. Only meaningful with
   *  `page === "files"`. */
  filePath?: string;
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
/** §73: the jobs page — every long-running daemon operation (backups,
 *  restores, publishes, installs) with its live state, progress, and its
 *  cancel verb. One per window; the daemon's history is authoritative. */
export interface JobsDestination {
  kind: "jobs";
}
/** §72: the audit page — the daemon's append-only trail of every mutating
 *  command and handshake, read back newest-first. */
export interface AuditDestination {
  kind: "audit";
}
/** §58: about — version, channel, and the panel's reserved rooms, stated
 *  rather than guessed. */
export interface AboutDestination {
  kind: "about";
}
/** §84's feedback room: the panel's own issue lane — an operator-typed
 *  report (title, details, an optional pasted screenshot) sent to the
 *  development repo, with the account and route stated on the page. */
export interface FeedbackDestination {
  kind: "feedback";
}
/** §56/§57: the extensions room — the daemon's inventory of installed
 *  extensions and the permissions they declare (deny-by-default). The
 *  execution/contribution model is reserved (ADR-0031) and the page
 *  says so. */
export interface ExtensionsDestination {
  kind: "extensions";
}
/** §58's reserved future URL, live now that a versioned channel exists
 *  (ADR-0029): the channel's published pre-releases, the installed
 *  build marked, manual downloads for trying a specific version. */
export interface DownloadsDestination {
  kind: "downloads";
}
/** The developer tools (Part 2): the control center over the protocol —
 *  console, servers, inspector, events. It uses the SAME client and
 *  stores as the rest of the UI, never a parallel implementation. */
export interface DevToolsDestination {
  kind: "devtools";
}
/** §7's join destination: a typed address the operator asked the shell
 *  to reach. The Join page (not a webview navigation) consults the
 *  daemon and speaks the verdict with its recovery paths. */
export interface JoinDestination {
  kind: "join";
  /** `undefined` is the port-only dialect — the local loopback. */
  host?: string;
  port: number;
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
  | DevToolsDestination
  | ServersDestination
  | NewTabDestination
  | ServerDestination
  | ConsoleDestination
  | JoinDestination
  | SettingsDestination
  | JobsDestination
  | AuditDestination
  | AboutDestination
  | FeedbackDestination
  | ExtensionsDestination
  | DownloadsDestination
  | MissingPageDestination;

/** A tab's stable identity. Derived from the destination on purpose: one
 *  tab per server, one fleet page, one new-tab page (§6). */
export type TabKey = string;

export const DEVTOOLS_TAB: TabKey = "devtools";
export const SERVERS_TAB: TabKey = "servers";
export const NEW_TAB: TabKey = "new";
export const SETTINGS_TAB: TabKey = "settings";
export const JOBS_TAB: TabKey = "jobs";
export const AUDIT_TAB: TabKey = "audit";
export const ABOUT_TAB: TabKey = "about";
export const FEEDBACK_TAB: TabKey = "feedback";
export const EXTENSIONS_TAB: TabKey = "extensions";
export const DOWNLOADS_TAB: TabKey = "downloads";
export const serverTab = (serverId: string): TabKey => `server:${serverId}`;
export const consoleTab = (serverId: string): TabKey => `console:${serverId}`;

export function tabKey(destination: Destination): TabKey {
  switch (destination.kind) {
    case "devtools":
      return DEVTOOLS_TAB;
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
    case "join":
      return `join:${destination.host ?? ""}:${destination.port}`;
    case "jobs":
      return JOBS_TAB;
    case "audit":
      return AUDIT_TAB;
    case "about":
      return ABOUT_TAB;
    case "feedback":
      return FEEDBACK_TAB;
    case "extensions":
      return EXTENSIONS_TAB;
    case "downloads":
      return DOWNLOADS_TAB;
    case "missing":
      return `missing:${destination.url}`;
  }
}

/** The canonical internal URL a destination is addressed by (§58). The
 *  INTERNAL spelling carries the server id; the ADDRESS BAR shows the
 *  operator's own words for the same place (restingAddress/visibleAddress)
 *  — identity and address stay two disciplines (§61). */
export function destinationUrl(destination: Destination): string {
  switch (destination.kind) {
    case "devtools":
      return "zim://devtools/";
    case "servers":
      return "zim://servers/";
    case "new":
      return "zim://new";
    case "settings":
      return "zim://settings/";
    case "jobs":
      return "zim://jobs/";
    case "audit":
      return "zim://audit/";
    case "about":
      return "zim://about/";
    case "feedback":
      return "zim://feedback/";
    case "extensions":
      return "zim://extensions/";
    case "downloads":
      return "zim://downloads/";
    case "server":
      return serverUrl(destination.serverId, destination.page, destination.filePath);
    case "console":
      return `zim://console/${destination.serverId}`;
    case "join":
      return `zim://join/${destination.host ?? ""}:${destination.port}`;
    case "missing":
      return destination.url;
  }
}

/** The internal URL of a server page — the round-trip target the bar's
 *  VISIBLE address re-commits to (parseAddressInput's join dialect with
 *  a path resolves back to exactly this). */
export function serverUrl(
  serverId: string,
  page?: ServerPage,
  filePath?: string,
): string {
  let url = `zim://server/${serverId}`;
  if (page) url += `/${page}`;
  if (page === "files" && filePath) url += `/${filePath.split("/").filter(Boolean).join("/")}`;
  return url;
}

/** What the address bar shows when the tab rests. A SERVER tab rests at
 *  the operator's own words for it (§8–§12's URL model): the join
 *  address, plus the page path, plus the files path under it —
 *  `localhost:25552/console`, `localhost:25552/files/eula.txt`. When the
 *  port is unknown the internal URL shows instead — an address the
 *  server cannot answer is not shown as if it could (§7's own law). */
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
  if (
    destination.kind === "jobs" ||
    destination.kind === "audit" ||
    destination.kind === "about" ||
    destination.kind === "feedback" ||
    destination.kind === "extensions" ||
    destination.kind === "downloads"
  ) {
    // The evidence pages rest at their internal URLs — they are ZIM
    // pages, not a server's join address.
    return destinationUrl(destination);
  }
  if (destination.kind === "console") {
    // The dedicated console tab rests at the operator's words for it too:
    // `host:port/console` (§9) — the same URL semantics every server
    // page carries. No port yet: the internal URL, honestly.
    const entry = entries.find((e) => e.serverId === destination.serverId);
    if (entry?.port) return `${joinAddress(entry, host)}/console`;
    return destinationUrl(destination);
  }
  if (destination.kind === "join") {
    // The join tab rests at the address the operator typed (§7).
    return destinationUrl(destination);
  }
  if (destination.kind === "devtools") {
    return destinationUrl(destination);
  }
  const entry = entries.find((e) => e.serverId === destination.serverId);
  if (entry?.port) {
    return `${joinAddress(entry, host)}${serverPath(destination.page, destination.filePath)}`;
  }
  return destinationUrl(destination);
}

/** The visible path suffix of a server page: `""`, `"/console"`,
 *  `/files/eula.txt` — the part after `host:port`. */
export function serverPath(page?: ServerPage, filePath?: string): string {
  if (!page) return "";
  if (page === "files" && filePath) {
    const clean = filePath.split("/").filter(Boolean).join("/");
    return clean === "" ? "/files" : `/files/${clean}`;
  }
  return `/${page}`;
}

/** The join address of a managed server. The daemon owns the port; the
 *  host is this window's honest reach: localhost for the local daemon,
 *  the box's host for a remote profile (the daemon's bind address is its
 *  own business — 0.0.0.0 and localhost reach the same local server). */
export function joinAddress(entry: ServerEntry, host: string): string {
  return `${host}:${entry.port}`;
}

/** The omnibox's DISPLAY law (Chrome's elision): a join tab rests at
 *  `zim://join/{host}:{port}`, but the operator thinks in `{host}:{port}`
 *  — the bar shows the address they would type, not the scheme, the way
 *  Chrome hides `https://www.`. Internal pages keep their zim:// spelling
 *  (chrome://settings shows its scheme too). Display-only: the shown
 *  string must re-commit to the same destination — parseAddressInput of a
 *  bare `host:port` IS the join dialect, so the round-trip holds. */
export function displayAddress(url: string): string {
  const prefix = "zim://join/";
  return url.startsWith(prefix) ? url.slice(prefix.length) : url;
}

/** The address bar's visible text for a RESTING URL in the server-address
 *  model (§8–§12): a server tab's internal URL translates to the join
 *  address + page path — `localhost:25552/console`, with the files path
 *  under it — and the dedicated console tab to `host:port/console` (§9).
 *  The fleet carries the ports (the favicon lane's projection); a server
 *  the lane has not learned yet, or one with no port, keeps its internal
 *  URL — an address it cannot answer is not shown as if it could. This
 *  is display-only: the same text re-commits through parseAddressInput's
 *  join dialect back to the same destination. */
export function visibleAddress(
  url: string,
  fleet: Record<string, { port?: number }> | undefined,
  host: string,
): string {
  const server = /^zim:\/\/server\/([^/?#]+)(?:\/([^/?#]+)(?:\/(.*))?)?$/.exec(url);
  if (server) {
    const id = decodeURIComponent(server[1] ?? "");
    const page = server[2];
    const rest = server[3];
    const port = fleet?.[id]?.port;
    if (port != null) {
      const suffix =
        page === undefined
          ? ""
          : rest === undefined
            ? `/${page}`
            : `/${page}/${rest.split("/").filter(Boolean).join("/")}`;
      return `${host}:${port}${suffix}`;
    }
    return url;
  }
  const console = /^zim:\/\/console\/([^/?#]+)$/.exec(url);
  if (console) {
    const id = decodeURIComponent(console[1] ?? "");
    const port = fleet?.[id]?.port;
    if (port != null) return `${host}:${port}/console`;
    return url;
  }
  return displayAddress(url);
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

// --- the address bar's dialects (§7, §8–§12) -----------------------------------------

export type AddressRequest =
  | { kind: "internal"; destination: Destination }
  | { kind: "join"; host?: string; port: number; page?: ServerPage; filePath?: string }
  | { kind: "query"; text: string };

const INTERNAL_URL = /^zim:\/\/([^/?#]+)\/?([^/?#]*)(?:\/?([^/?#]*)(?:\/(.*))?)?$/;
/** `host:port`, `:port`, or a bare port — the join dialect's authority
 *  half. A host may be an IPv4, a hostname, or a bracketed IPv6; the
 *  port is 1-65535. */

/** Split a typed line into its authority half and its path half on the
 *  FIRST `/` (a join address never contains one — IPv6 brackets carry
 *  colons, never slashes). `localhost:25552/console` answers
 *  `["localhost:25552", ["console"]]`; a pathless line answers `[]`. */
function splitJoinPath(text: string): [string, string[]] {
  const at = text.indexOf("/");
  if (at === -1) return [text, []];
  return [text.slice(0, at), text.slice(at + 1).split("/").filter(Boolean)];
}
const JOIN = /^(?:\[?([a-zA-Z0-9._-]+|\[[0-9a-fA-F:]+\])\]?)?:(\d{1,5})$|^\[?([a-zA-Z0-9._-]+)\]?$/;

/** Whether the word parses as an IPv6 literal — the exact twin of the
 *  Rust side's `Ipv6Addr::from_str` (classify's IPv6 law), because no
 *  stdlib parse exists here. The RFC's own rules: 1-4 hex digits per
 *  group, exactly 8 groups, or ONE "::" elision covering at least one
 *  group; an optional dotted-quad tail rides the last group (the
 *  "::ffff:127.0.0.1" spelling) and counts as TWO groups. A host:port
 *  spelling never passes ("box:25565" has non-hex chars, "0:25565"'s
 *  port exceeds a 16-bit group), so the join dialect keeps its words. */
export function isIpv6Shaped(word: string): boolean {
  // Dots ride the gate so the dotted-quad tail below can speak — the
  // group checks are the real law, a dot can never pass one.
  if (!/^[0-9a-fA-F:.]+$/.test(word)) return false;
  const halves = word.split("::");
  if (halves.length > 2) return false; // at most one elision
  const elided = halves.length === 2;
  const head = halves[0] ?? "";
  const headGroups = head === "" ? [] : head.split(":");
  // The tail side only exists across an elision — without "::" the
  // whole word is the head (the old indexing double-counted it).
  const tail = elided ? (halves[1] ?? "") : "";
  const tailGroups = tail === "" ? [] : tail.split(":");
  // The dotted-quad tail: the last group may be an IPv4 ending — it
  // rides the address as TWO groups' worth of bits.
  let ipv4Tail = false;
  const last = tailGroups[tailGroups.length - 1];
  if (last && last.includes(".")) {
    if (!/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.test(last)) return false;
    if (last.split(".").some((o) => Number(o) > 255)) return false;
    tailGroups.pop();
    ipv4Tail = true;
  }
  const groups = headGroups.length + tailGroups.length + (ipv4Tail ? 2 : 0);
  if (elided && groups >= 8) return false; // the elision must cover ≥1
  if (!elided && groups !== 8) return false;
  return [...headGroups, ...tailGroups].every(
    (g) => /^[0-9a-fA-F]{1,4}$/.test(g),
  );
}

/** The tail of a PRODUCED join URL (destinationUrl() writes
 *  `zim://join/{host}:{port}` — the omnibox rests on that string, so a
 *  re-commit must navigate, not fall to Missing; the user's own
 *  "No page at zim://join/…" screenshot was exactly that fall). The last
 *  colon is the port separator, so an IPv6 host keeps its colons; the
 *  host spelling stays raw so url() → parse → url() round-trips
 *  exactly (§61: identity preserved). A tail with no colon is not a URL
 *  destinationUrl() can produce — null, and the caller answers Missing. */
export function parseJoinTail(tail: string): JoinDestination | null {
  const sep = tail.lastIndexOf(":");
  if (sep === -1) return null;
  const port = Number(tail.slice(sep + 1));
  if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
  const host = tail.slice(0, sep);
  return host === "" ? { kind: "join", port } : { kind: "join", host, port };
}

/** The typed line's path half, resolved against the page vocabulary:
 *  `["console"]` → page "console"; `["files", "eula.txt"]` → page
 *  "files" + filePath "eula.txt"; `["files", "tmp"]` → page "files" +
 *  "tmp". An EMPTY path list answers all-absent (the dashboard root). A
 *  first segment outside the vocabulary answers `null` — the line is
 *  not a server URL, and the caller owes the operator the honest
 *  "no page" answer, never a silent dashboard. */
function parseServerPath(segments: string[]): { page?: ServerPage; filePath?: string } | null {
  if (segments.length === 0) return {};
  const [first, ...rest] = segments;
  if (!first || !isServerPage(first)) return null;
  if (first === "files" && rest.length > 0) {
    return { page: "files", filePath: rest.join("/") };
  }
  return { page: first };
}

/** Parse what the operator typed, before any store is consulted. The
 *  join dialect now carries the server-page path (§8–§12): the authority
 *  half (`host:port`) resolves the registry's port, the path half names
 *  the page under it — `0.0.0.0:25552/console`, `0.0.0.0:25552/files/
 *  eula.txt`. A path whose first word is not a page is not this dialect
 *  (the caller answers honestly); a page-less line is the root join. */
export function parseAddressInput(text: string): AddressRequest {
  const trimmed = text.trim();
  const internal = INTERNAL_URL.exec(trimmed);
  if (internal) {
    const page = internal[1];
    if (page === "servers") return { kind: "internal", destination: { kind: "servers" } };
    if (page === "new") return { kind: "internal", destination: { kind: "new" } };
    if (page === "settings") return { kind: "internal", destination: { kind: "settings" } };
    if (page === "jobs") return { kind: "internal", destination: { kind: "jobs" } };
    if (page === "audit") return { kind: "internal", destination: { kind: "audit" } };
    if (page === "about") return { kind: "internal", destination: { kind: "about" } };
    if (page === "feedback") return { kind: "internal", destination: { kind: "feedback" } };
    if (page === "extensions") return { kind: "internal", destination: { kind: "extensions" } };
    if (page === "downloads") return { kind: "internal", destination: { kind: "downloads" } };
    if (page === "server" && internal[2]) {
      // zim://server/{id}/{page}/{file...} — the internal spelling of the
      // server-page model. The id is the first segment; the rest is the
      // same page dialect the visible addresses carry.
      const segments = trimmed.slice(`zim://server/`.length).split("/").filter(Boolean);
      const id = segments[0];
      const rest = segments.slice(1);
      if (id && rest.length === 0) {
        return { kind: "internal", destination: { kind: "server", serverId: id } };
      }
      const path = parseServerPath(rest);
      if (id && path) {
        return {
          kind: "internal",
          destination: path.page
            ? { kind: "server", serverId: id, ...path }
            : { kind: "server", serverId: id },
        };
      }
      return { kind: "internal", destination: { kind: "missing", url: trimmed } };
    }
    if (page === "console" && internal[2]) {
      return { kind: "internal", destination: { kind: "console", serverId: internal[2] } };
    }
    // The join URL dialect — the closed loop with destinationUrl(): every
    // URL a join tab can rest on must re-navigate to that tab.
    if (page === "join" && internal[2]) {
      const parsed = parseJoinTail(internal[2]);
      return parsed
        ? { kind: "internal", destination: parsed }
        : { kind: "internal", destination: { kind: "missing", url: trimmed } };
    }
    // An unknown internal page is a real destination request the shell
    // answers honestly (no such page) — not silently a web search.
    return { kind: "internal", destination: { kind: "missing", url: trimmed } };
  }
  // The IPv6 literal law (Chrome's fixup): the brackets are input sugar
  // — "::1" and "[::1]" both join. Runs before the dialect so a bare
  // address is never mistaken for host:port words; the dialect's own
  // spellings never parse as addresses, so nothing else moves. An IPv6
  // literal never carries a path (no `/` can appear inside one that
  // survives the bracket law), so the whole line is the address.
  const unbracketed = trimmed.startsWith("[")
    ? trimmed.endsWith("]")
      ? trimmed.slice(1, -1)
      : trimmed.slice(1)
    : trimmed;
  if (isIpv6Shaped(unbracketed)) {
    return { kind: "join", host: normalizeHost(trimmed), port: 0 };
  }
  // The path dialect: split the authority half off first, then run the
  // join dialect on it — `0.0.0.0:25552/console` is a join WITH a path,
  // never a query. Only a page-shaped path qualifies; anything else
  // falls through to the honest dialects below (a word with a slash is
  // a search, the same as before).
  const [authority, pathSegments] = splitJoinPath(trimmed);
  const join = JOIN.exec(authority);
  if (join) {
    if (join[2]) {
      const port = Number(join[2]);
      if (port >= 1 && port <= 65_535) {
        const path = parseServerPath(pathSegments);
        if (path) {
          return { kind: "join", host: normalizeHost(join[1]), port, ...path };
        }
        if (pathSegments.length > 0) {
          // A path was typed but it names no page: an honest miss, never
          // a silent dashboard or a masked search.
          return { kind: "internal", destination: { kind: "missing", url: trimmed } };
        }
        return { kind: "join", host: normalizeHost(join[1]), port };
      }
    } else if (join[3]) {
      // A bare word could be a port ("25565") — only digits qualify.
      if (/^\d{1,5}$/.test(join[3])) {
        const port = Number(join[3]);
        if (port >= 1 && port <= 65_535) {
          const path = parseServerPath(pathSegments);
          if (path && pathSegments.length > 0) {
            return { kind: "join", port, ...path };
          }
          if (pathSegments.length > 0) {
            return { kind: "internal", destination: { kind: "missing", url: trimmed } };
          }
          return { kind: "join", port };
        }
      } else if (
        join[3].includes(".") ||
        join[3].toLowerCase() === "localhost"
      ) {
        // The bare-word law (Chrome's fixup posture): a host-looking
        // word navigates — the dot is the host's own signal — and
        // "localhost" is navigable by its name (Chrome's exception, and
        // the most common join of all). A plain word is a discovery
        // search: "paper" must never join a host called "paper". The
        // word carries no port — 0, and the Join page speaks the
        // verdict (§7: nothing is invented). A PATH on a portless
        // host names no server page (the page model resolves by port):
        // the line reads as a URL and lands as an honest miss, never a
        // silent join that drops the operator's words.
        if (pathSegments.length > 0) {
          return { kind: "internal", destination: { kind: "missing", url: trimmed } };
        }
        return { kind: "join", host: normalizeHost(join[3]), port: 0 };
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

/** The local aliases a typed host may use for "this machine" — §7's
 *  join dialect, including the founder's own `0` shorthand for
 *  bind-all. A remote profile's box address resolves the same way:
 *  the host hint is the box itself. */
function isLocalAlias(host: string, hint: string): boolean {
  const normalized = host.toLowerCase();
  return (
    normalized === "localhost" ||
    normalized === "0" ||
    normalized === "0.0.0.0" ||
    normalized === "127.0.0.1" ||
    normalized === "::" ||
    normalized === "::1" ||
    normalized === hint.toLowerCase()
  );
}

/** Resolve a join request against the registry (P0 §13–§14). The port is
 *  the match; the host must agree with the server's bind: an all- or
 *  unset-bind server answers this machine's aliases, a specifically
 *  bound server answers its own address. Nothing is invented — an
 *  unknown destination stays unknown (§16). */
export function resolveJoin(
  request: { host?: string; port: number },
  entries: ServerEntry[],
  host: string,
): ServerEntry | null {
  const typedHost = request.host;
  const bindSays = (entry: ServerEntry): boolean => {
    const bind = entry.bindAddress?.trim() ?? "";
    if (bind === "" || bind === "0.0.0.0" || bind === "::" || bind === "*") {
      // All interfaces: this machine's own aliases reach it.
      return typedHost === undefined || isLocalAlias(typedHost, host);
    }
    if (typedHost === undefined) return true;
    return bind.toLowerCase() === typedHost.toLowerCase();
  };
  const matches = entries.filter((e) => e.port === request.port && bindSays(e));
  if (matches.length === 0) return null;
  if (matches.length === 1) return matches[0] ?? null;
  // Several servers agree (possible across boxes in future remote
  // fleets): prefer the exact host spelling, else say nothing yet.
  const exact = matches.find(
    (e) => joinAddress(e, host) === `${request.host ?? host}:${request.port}`,
  );
  return exact ?? null;
}

/** The human label a destination deserves — the bookmark star's default
 *  name and the bookmark bar's fallback chip text. A server keeps its
 *  display name (identity preserved, §61); internal pages keep their
 *  page names. */
export function destinationLabel(destination: Destination, entries: ServerEntry[]): string {
  const name = (serverId: string): string =>
    entries.find((entry) => entry.serverId === serverId)?.displayName ?? serverId;
  switch (destination.kind) {
    case "server":
      // The page rides the label the way a browser tab carries its page
      // title: the server stays the identity, the page says where in the
      // workspace the tab rests.
      return destination.page
        ? `${name(destination.serverId)} — ${destination.page}`
        : name(destination.serverId);
    case "console":
      return `${name(destination.serverId)} console`;
    case "join":
      return `${destination.host ?? "port"}:${destination.port}`;
    case "devtools":
      return "Developer tools";
    case "servers":
      return "Servers";
    case "settings":
      return "Settings";
    case "jobs":
      return "Jobs";
    case "audit":
      return "Audit log";
    case "about":
      return "About ZIM";
    case "feedback":
      return "Feedback";
    case "extensions":
      return "Extensions";
    case "downloads":
      return "Downloads";
    case "new":
      return "New tab";
    case "missing":
      return destination.url;
  }
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
