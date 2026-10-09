// zim://servers/ — the fleet page (ADR-0015). The registry as
// cards, honest counts, the daemon identity. The empty state keeps the
// original nudge sentence (it is the documented §53 journey copy) and
// gives it the space it deserves.
//
// Live: the cards' states ride the events stream (ADR-0006) — this page
// renders the authoritative store, never a snapshot copy. The filter is
// instant and matches both the Server ID and the display name (the
// operator's alias).

import { useMemo, useState } from "react";
import { useConnection } from "../state/connection";
import type { ServerEntry } from "../state/servers";
import { Button } from "../ui/Button";
import { StatusChip } from "../ui/StatusChip";
import { IconBolt, IconPlus, IconSearch, IconServer } from "../ui/icons";
import styles from "./FleetPage.module.css";

export function FleetPage({
  servers,
  onOpen,
  onNewServer,
}: {
  servers: ServerEntry[];
  onOpen: (serverId: string) => void;
  onNewServer: () => void;
}) {
  const status = useConnection((s) => s.status);
  const daemon = useConnection((s) => s.daemon);
  const [filter, setFilter] = useState("");
  // Instant filtering by Server ID and alias: a lowercase substring over
  // both fields, applied per keystroke — no daemon round-trip, the cards
  // themselves are the live state.
  const filtered = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (needle === "") return servers;
    return servers.filter(
      (server) =>
        server.serverId.toLowerCase().includes(needle) ||
        server.displayName.toLowerCase().includes(needle),
    );
  }, [servers, filter]);
  const running = servers.filter((s) => s.state === "running").length;
  const live = servers.filter((s) =>
    ["starting", "stopping", "adopting"].includes(s.state),
  ).length;

  return (
    <div className={styles.page}>
      <header className={styles.pageHead}>
        <div>
          <h1 className={styles.title}>Servers</h1>
          <p className={styles.subtitle}>
            {status === "ready"
              ? daemon
                ? `ZIM v${daemon.version} is online — ${running} of ${servers.length} running`
                : "ZIM is online"
              : "Waiting for ZIM…"}
          </p>
        </div>
        <Button variant="primary" onClick={onNewServer}>
          <IconPlus size={14} />
          New server
        </Button>
      </header>

      {servers.length > 0 ? (
        <>
          <div className={styles.stats}>
            <div className={styles.stat}>
              <span className={styles.statIcon}>
                <IconServer size={18} />
              </span>
              <div className={styles.statBody}>
                <span className={styles.statValue}>{servers.length}</span>
                <span className={styles.statLabel}>Servers</span>
              </div>
            </div>
            <div className={styles.stat}>
              <span className={`${styles.statIcon} ${styles.statIconLive}`}>
                <IconBolt size={18} />
              </span>
              <div className={styles.statBody}>
                <span className={styles.statValue}>{running}</span>
                <span className={styles.statLabel}>Running</span>
              </div>
            </div>
            <div className={styles.stat}>
              <span className={styles.statIcon}>
                <IconSearch size={18} />
              </span>
              <div className={styles.statBody}>
                <span className={styles.statValue}>{live}</span>
                <span className={styles.statLabel}>In transition</span>
              </div>
            </div>
          </div>

          <h2 className={styles.sectionTitle}>Fleet</h2>
          {servers.length > 3 ? (
            <div className={styles.filterRow}>
              <span className={styles.filterIcon}>
                <IconSearch size={14} />
              </span>
              <input
                className={styles.filterInput}
                value={filter}
                placeholder="Filter by server ID or name"
                aria-label="Filter servers by ID or name"
                spellCheck={false}
                onChange={(event) => setFilter(event.target.value)}
              />
              {filter !== "" ? (
                <button
                  className={styles.filterClear}
                  aria-label="Clear filter"
                  onClick={() => setFilter("")}
                >
                  ×
                </button>
              ) : null}
            </div>
          ) : null}
          {filtered.length === 0 && filter !== "" ? (
            <p className={styles.filterEmpty} role="status">
              No server matches “{filter}”. The filter matches the Server ID and the display name.
            </p>
          ) : (
            <div className={styles.grid}>
              {filtered.map((server) => (
              <div
                key={server.serverId}
                className={styles.card}
                onClick={() => onOpen(server.serverId)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") onOpen(server.serverId);
                }}
                tabIndex={0}
                role="button"
                aria-label={`Open ${server.displayName}`}
              >
                <div className={styles.cardTop}>
                  <span className={styles.cardName}>{server.displayName}</span>
                  <StatusChip state={server.state} />
                </div>
                <div className={styles.cardMeta}>
                  <span className={styles.cardId}>{server.serverId}</span>
                  {server.software ? <span className={styles.cardChip}>{server.software}</span> : null}
                  {server.version ? <span className={styles.cardChip}>v{server.version}</span> : null}
                  {server.port ? <span className={styles.cardChip}>:{server.port}</span> : null}
                </div>
                <div className={styles.cardFoot}>
                  <span className={styles.cardOpen}>Open →</span>
                </div>
              </div>
            ))}
            </div>
          )}
        </>
      ) : (
        <div className={styles.empty}>
          <span className={styles.emptyIcon}>
            <IconServer size={28} />
          </span>
          <h2 className={styles.emptyTitle}>No servers yet.</h2>
          <p className={styles.emptyBody}>
            Use “+ New server” (or the Ctrl+K palette) to register an existing server directory.
          </p>
          <Button variant="primary" onClick={onNewServer}>
            <IconPlus size={14} />
            New server
          </Button>
        </div>
      )}
    </div>
  );
}
