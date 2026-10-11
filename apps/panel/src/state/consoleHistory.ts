// §26's other half: the composer's command recall — every terminal since
// forever has Up/Down. Per-server history (a Survival server's vocabulary
// is not a creative test server's), PANEL-LOCAL like the connection
// profiles, persisted to localStorage and capped at 100 entries so months
// of muscle memory never becomes an unbounded key.
//
// Each tab webview is its own JS realm: same-realm writes publish through
// a module emitter, cross-realm writes ride the storage event (the same
// lane the favicon dot and the connection profiles ride) — so a command
// sent in the dedicated console tab is immediately recallable from the
// embedded console of the same server, and vice versa.

import { useSyncExternalStore } from "react";

const KEY = "zamin.consoleHistory";
export const HISTORY_CAP = 100;

const EMPTY: string[] = [];

function isRecord(value: unknown): value is Record<string, unknown> {
  return value instanceof Object && !Array.isArray(value);
}

/** Parse defensively: the key is panel-local and versionless, so a
 *  corrupted or hand-edited payload degrades to "no history", never a
 *  thrown render. Entries are validated per server and re-capped. */
function readAll(): Record<string, string[]> {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw === null) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) return {};
    const out: Record<string, string[]> = {};
    for (const [serverId, entries] of Object.entries(parsed)) {
      if (!Array.isArray(entries)) continue;
      const commands = entries.filter(
        (entry): entry is string => typeof entry === "string" && entry.trim() !== "",
      );
      if (commands.length > 0) out[serverId] = commands.slice(0, HISTORY_CAP);
    }
    return out;
  } catch {
    return {};
  }
}

function writeAll(all: Record<string, string[]>): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    // A blocked or full storage keeps this session's recall in the
    // emitter cache — the console works, persistence degrades quietly.
  }
}

// The realm's snapshot cache: useSyncExternalStore requires a stable
// identity between publishes, so reads are memoized until a write (same
// realm) or a storage event (another realm) invalidates them.
let cache: Record<string, string[]> | null = null;
const listeners = new Set<() => void>();

function snapshot(): Record<string, string[]> {
  if (cache === null) cache = readAll();
  return cache;
}

function publish(): void {
  cache = null;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

// Another realm wrote the key (storage events never fire in the realm
// that wrote). A key of null means storage was cleared wholesale.
if (typeof window !== "undefined") {
  window.addEventListener("storage", (event: StorageEvent) => {
    if (event.key === KEY || event.key === null) publish();
  });
}

/** The plain read — tests and non-reactive callers. Oldest entry last. */
export function consoleHistoryFor(serverId: string): string[] {
  return snapshot()[serverId] ?? EMPTY;
}

/** Record a sent command. A consecutive duplicate collapses (the operator
 *  re-sent the same line — bash's law); re-sending an older entry moves
 *  it to the front (the recall the operator actually repeats stays one
 *  Up-press away). The cap evicts from the oldest end. */
export function recordCommand(serverId: string, command: string): void {
  const trimmed = command.trim();
  if (trimmed === "") return;
  const all = { ...snapshot() };
  const prior = all[serverId] ?? [];
  all[serverId] = [trimmed, ...prior.filter((entry) => entry !== trimmed)].slice(
    0,
    HISTORY_CAP,
  );
  writeAll(all);
  publish();
}

/** The reactive read for the composer. */
export function useConsoleHistory(serverId: string): string[] {
  return useSyncExternalStore(
    subscribe,
    () => snapshot()[serverId] ?? EMPTY,
    () => EMPTY,
  );
}
