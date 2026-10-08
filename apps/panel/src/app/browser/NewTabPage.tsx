// The new tab (ADR-0015): a blank browser-style page whose center holds a
// single input. It is not a web search box — it speaks the same three
// dialects as the address bar, and free text is a discovery query over the
// registry (§22's deterministic form). Below it, the servers this panel
// knows, active first, each with its opening action. The Dutchmen
// transition that eventually lives here is a reserved room.

import { useEffect, useState } from "react";
import { parseAddressInput, joinAddress, searchServers } from "../../state/destinations";
import { registerServer } from "../../state/actions";
import type { DiscoveredServer } from "../../protocol/types";
import { sortedServers, useServers } from "../../state/servers";
import { useTabs } from "../../state/tabs";
import { useUi } from "../../state/ui";
import { Button } from "../../ui/Button";
import { StatusChip } from "../../ui/StatusChip";
import { StatusDot } from "../../ui/StatusDot";
import { IconPlus, IconServer } from "../../ui/icons";
import { commitAddress } from "./commitAddress";
import { MachineDiscovery, slugFromPath } from "./machineDiscovery";
import { describeError } from "../../state/errors";
import { ErrorNote } from "../../ui/ErrorNote";
import styles from "./NewTabPage.module.css";

const ACTIVE_STATES = new Set(["running", "starting", "adopting"]);

export function NewTabPage() {
  const query = useTabs((s) => s.discoveryQuery);
  const setDiscoveryQuery = useTabs((s) => s.setDiscoveryQuery);
  const setNewServerOpen = useUi((s) => s.setNewServerOpen);
  const serverMap = useServers((s) => s.servers);
  const entries = sortedServers(serverMap);
  // This window's reach: localhost for the local daemon, the box's host
  // for a remote profile — the same rule the address bar rests by.
  const host = "localhost";

  // The query dialect lands here: consume the carried query once, then
  // the input is the page's own state. Filtering is live — the results
  // follow the text as it is typed; Enter only matters for the address
  // dialects.
  const [text, setText] = useState("");
  useEffect(() => {
    if (query !== null) {
      setText(query);
      setDiscoveryQuery(null);
    }
  }, [query, setDiscoveryQuery]);

  // The machine's answer (§64): what the scan holds beyond the registry,
  // following the same text. The open verb turns a directory into a
  // managed server (register with the folder's slug) and opens its tab;
  // a refusal is a typed note, never a fake navigation.
  const [openNote, setOpenNote] = useState<string | null>(null);
  const openCandidate = async (candidate: DiscoveredServer) => {
    if (candidate.kind !== "directory") {
      setOpenNote(
        "That is a jar, not a server directory — opening it starts nothing. Register the folder that runs it first.",
      );
      return;
    }
    const serverId = slugFromPath(candidate.path);
    const displayName = candidate.displayName ?? serverId;
    try {
      await registerServer({ serverId, displayName, rootPath: candidate.path });
      useTabs.getState().navigate({ kind: "server", serverId });
    } catch (error) {
      const described = describeError(error);
      setOpenNote(`${described.title} (${candidate.path})`);
    }
  };

  const onSubmit = (event: React.FormEvent) => {
    event.preventDefault();
    const request = parseAddressInput(text);
    if (request.kind === "internal" || request.kind === "join") {
      commitAddress(text, entries, host);
    }
    // Free text needs no submit: the results already follow the typing,
    // and a search refinement is not a navigation.
  };

  const filter = text.trim();
  const hits = filter === "" ? entries : searchServers(filter, entries);
  const ordered = [
    ...hits.filter((e) => ACTIVE_STATES.has(e.state)),
    ...hits.filter((e) => !ACTIVE_STATES.has(e.state)),
  ];

  return (
    <div className={styles.page}>
      <div className={styles.center}>
        <h1 className={styles.title}>Find a server</h1>
        <form className={styles.form} onSubmit={onSubmit} role="search">
          <input
            className={styles.input}
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Search servers, or type localhost:25565"
            aria-label="Search servers or type an address"
            spellCheck={false}
            autoComplete="off"
          />
        </form>
        <p className={styles.hint}>
          A name finds servers here. An address like{" "}
          <code className={styles.code}>localhost:25565</code> opens one. Internal pages live under{" "}
          <code className={styles.code}>zaminpanel://</code>.
        </p>

        {entries.length === 0 ? (
          <div className={styles.empty}>
            <span className={styles.emptyIcon}>
              <IconServer size={26} />
            </span>
            <h2 className={styles.emptyTitle}>No servers registered yet.</h2>
            <p className={styles.emptyBody}>
              Register an existing server directory, or create one from the catalog.
            </p>
            <Button variant="primary" onClick={() => setNewServerOpen(true)}>
              <IconPlus size={14} />
              New server
            </Button>
          </div>
        ) : filter !== "" && hits.length === 0 ? (
          <p className={styles.noHits} role="status">
            No server matches “{filter}”.
          </p>
        ) : (
          <ul className={styles.list}>
            {ordered.map((server) => (
              <li key={server.serverId}>
                <button
                  className={styles.row}
                  onClick={() =>
                    commitAddress(
                      server.port
                        ? joinAddress(server, host)
                        : `zaminpanel://server/${server.serverId}`,
                      entries,
                      host,
                    )
                  }
                  aria-label={`Open ${server.displayName}`}
                >
                  <StatusDot state={server.state} />
                  <span className={styles.name}>{server.displayName}</span>
                  <span className={styles.addr}>
                    {server.port ? joinAddress(server, host) : server.serverId}
                  </span>
                  <StatusChip state={server.state} />
                </button>
              </li>
            ))}
          </ul>
        )}

        {openNote ? (
          <div className={styles.machine} role="alert">
            <ErrorNote
              error={{
                title: openNote,
                remediation: ["The registry is unchanged — nothing was half-opened."],
              }}
            />
          </div>
        ) : null}
        <MachineDiscovery query={filter} onOpen={(candidate) => void openCandidate(candidate)} />
      </div>
    </div>
  );
}
