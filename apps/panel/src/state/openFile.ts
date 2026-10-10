// The OS handoff lane (founder §3/§4): double-clicking a supported server
// JAR opens it in ZIM. The host forwards the path — the single-instance
// argv when ZIM is already running, the command line at cold start — and
// the content layer decides what the path MEANS. This module is that
// decision, kept pure and testable: what ZIM may conclude about a path,
// never a guess dressed as a conclusion.
//
// The founder's own law (§3): "Do not identify a server merely because a
// file is called server.jar." The evidence comes from the daemon's
// discovery layer (ADR-0027) — the same one the New Tab page rides — so
// the double-click verdict and the operator's own scan agree by
// construction:
//
//   parent directory already registered → open that server's tab
//   parent directory server-shaped      → register it, then open the tab
//   anything else                       → the New Tab page with the
//                                         discovery query pre-filled —
//                                         the machine's honest answer
//                                         about that folder, and the
//                                         operator's hand makes the
//                                         call ZIM refuses to make
//
// The last row is deliberate: a jar lying in Downloads must not make ZIM
// register Downloads as a server root. Registering a directory is an
// operator decision (the New Server form's register door, or the
// discovery row's own action); the file association only opens what is
// already a server.

import { discoverServers, listServers, registerServer } from "./actions";
import type { DiscoveredServer } from "../protocol/types";
import { useServers } from "./servers";
import { invokeHost } from "../integration/tauri";
import { CMD } from "../commandIds";

/** One plan for one OS-handed path. */
export type JarOpenPlan =
  | { kind: "open"; serverId: string }
  | {
      kind: "register";
      serverId: string;
      displayName: string;
      rootPath: string;
    }
  | { kind: "discover"; query: string };

/** Path comparison that survives the OS's mood: separators fold to `/`,
 *  case folds (Windows and macOS are case-insensitive; Linux paths that
 *  differ only by case are vanishingly rare server roots), trailing
 *  separators drop. */
function samePath(a: string, b: string): boolean {
  const fold = (p: string): string =>
    p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  return fold(a) === fold(b);
}

/** The last path segment (`a/b/c` → `c`; `C:\Servers\SMP` → `SMP`). */
function baseName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/").filter((part) => part !== "");
  return parts.at(-1) ?? "";
}

/** The parent directory (`a/b/c` → `a/b`; `C:\x\y` → `C:\x`). */
function parentDir(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  parts.pop();
  return parts.join("/");
}

/** A server id the registry will accept, derived from the directory's
 *  own name: lowercase, `[a-z0-9-]`, collapsed dashes, capped at the
 *  protocol's 64, unique against the live registry (NewServerModal's
 *  validateServerId states the client-side rule; the daemon owns the
 *  authoritative one — a race that still collides surfaces as the typed
 *  error and the caller falls back to the discovery page). */
export function serverIdFromDirectoryName(
  name: string,
  taken: ReadonlySet<string>,
): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 64);
  const base = slug.length > 0 && /^[a-z0-9]/.test(slug) ? slug : "server";
  if (!taken.has(base)) return base;
  for (let n = 2; n < 10_000; n += 1) {
    const next = `${base}-${n}`.slice(0, 64);
    if (!taken.has(next)) return next;
  }
  return `server-${Date.now()}`;
}

/** Strip the quoting habits of Windows shell association (`"C:\a
 *  b\paper.jar"` may arrive with stray quotes or trailing whitespace). */
export function normalizeOsPath(raw: string): string {
  let path = raw.trim();
  while (path.startsWith('"') && path.endsWith('"') && path.length >= 2) {
    path = path.slice(1, -1);
  }
  return path.trim();
}

/** The verdict for one OS-handed path, from the daemon's own discovery
 *  answer and the live registry. Pure — the caller owns every effect. */
export function planJarOpen(
  rawPath: string,
  discovered: ReadonlyArray<DiscoveredServer>,
  registeredIds: ReadonlySet<string>,
): JarOpenPlan {
  const path = normalizeOsPath(rawPath);
  const parent = parentDir(path);
  const directory = baseName(parent);

  // Not a jar (or not a path at all): no server story to tell. The
  // discovery page scoped to the folder is the honest landing — it says
  // what the machine actually knows about that directory.
  if (!path.toLowerCase().endsWith(".jar")) {
    return { kind: "discover", query: parent };
  }

  // A managed server already owns the parent directory: open it. The
  // `registered` row's path is the server root; the marker (bound at
  // registration time, ADR-0004) names the owner even when the registry
  // row itself came back under `directory` kind.
  const registered = discovered.find(
    (entry) => entry.kind === "registered" && samePath(entry.path, parent),
  );
  if (registered?.serverId && registeredIds.has(registered.serverId)) {
    return { kind: "open", serverId: registered.serverId };
  }
  const shaped = discovered.find(
    (entry) => entry.kind === "directory" && samePath(entry.path, parent),
  );
  if (shaped?.marker && registeredIds.has(shaped.marker)) {
    return { kind: "open", serverId: shaped.marker };
  }

  // A server-shaped directory nobody owns: register it under an id
  // derived from its own name, then open the tab. The discovery row's
  // path is the daemon's canonical spelling of the root — trust it over
  // the OS's raw argv.
  if (shaped) {
    return {
      kind: "register",
      serverId: serverIdFromDirectoryName(directory, registeredIds),
      displayName: directory.length > 0 ? directory : shaped.path,
      rootPath: shaped.path,
    };
  }

  // A supported jar standing alone (its parent is not server-shaped), or
  // a jar discovery never classified: registering the parent would annex
  // whatever folder the jar happens to sit in. The discovery page with
  // the folder as its query is the truthful next step — the operator
  // sees exactly what the machine sees and decides.
  return { kind: "discover", query: parent };
}

/** Open a new tab at a destination (the content layer's one verb into
 *  the strip's NEW_TAB, with the optional waiting-search query the
 *  omnibox's Alt-Enter lane already speaks). */
async function newTab(
  destination: Record<string, unknown>,
  query?: string,
): Promise<void> {
  await invokeHost("shell_command", {
    id: CMD.NEW_TAB,
    arg: query === undefined ? { destination } : { destination, query },
  });
}

/** The full handoff: resolve the path, then act. One path, one landing
 *  place, every failure honest — a failed registration or a dead wire
 *  falls back to the discovery page scoped to the folder, so the
 *  operator always lands somewhere that explains itself. */
export async function openServerFile(rawPath: string): Promise<void> {
  const path = normalizeOsPath(rawPath);
  let plan: JarOpenPlan;
  try {
    const [discover, list] = await Promise.all([
      discoverServers(path),
      listServers(),
    ]);
    const registeredIds = new Set(
      list.servers.map((server) => server.serverId),
    );
    plan = planJarOpen(path, discover.servers, registeredIds);
  } catch {
    // No discovery answer (daemon down, wire dead): the discovery page
    // carries the folder as its query and says so itself.
    plan = { kind: "discover", query: parentDir(path) };
  }

  if (plan.kind === "open") {
    await newTab({ kind: "server", serverId: plan.serverId });
    return;
  }
  if (plan.kind === "register") {
    try {
      const result = await registerServer({
        serverId: plan.serverId,
        displayName: plan.displayName,
        rootPath: plan.rootPath,
      });
      // The events stream reconciles the same id; the store seed makes
      // the tab's first paint true even if the event races the reply.
      useServers.getState().upsert(result.server);
      await newTab({ kind: "server", serverId: plan.serverId });
      return;
    } catch {
      plan = { kind: "discover", query: parentDir(path) };
    }
  }
  await newTab({ kind: "new" }, plan.query);
}
