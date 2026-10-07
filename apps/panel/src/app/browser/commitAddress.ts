// The one address-commit path (ADR-0015): both the tool bar's address bar
// and the new tab's discovery input resolve what the operator typed through
// this function, so the bar's dialects behave identically everywhere. The
// caller owns presentation (the miss note, the blur) — this owns navigation.

import { parseAddressInput, resolveJoin } from "../../state/destinations";
import type { ServerEntry } from "../../state/servers";
import { useTabs } from "../../state/tabs";

export type CommitOutcome =
  | { kind: "navigated" }
  | { kind: "join-miss"; port: number }
  | { kind: "query"; text: string };

export function commitAddress(
  text: string,
  entries: ServerEntry[],
  host: string,
): CommitOutcome {
  const request = parseAddressInput(text);
  switch (request.kind) {
    case "internal":
      useTabs.getState().navigate(request.destination);
      return { kind: "navigated" };
    case "join": {
      const hit = resolveJoin(request, entries, host);
      if (!hit) return { kind: "join-miss", port: request.port };
      useTabs.getState().navigate({ kind: "server", serverId: hit.serverId });
      return { kind: "navigated" };
    }
    case "query":
      // A discovery query lands on the new tab (§22's deterministic form):
      // focused if it already rests, created if it does not.
      useTabs.getState().newTab(request.text);
      return { kind: "query", text: request.text };
  }
}
