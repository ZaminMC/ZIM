// Overview dashboard: the landing page when no server workspace is open.
// Honest numbers only — what the panel actually knows (fleet counts and
// the daemon identity), then the fleet itself as cards. The empty state
// keeps the original nudge sentence (it is the documented §53 journey
// copy) and gives it the space it deserves.

import { useConnection } from "../state/connection";
import type { ServerEntry } from "../state/servers";
import { Button } from "../ui/Button";
import { StatusChip } from "../ui/StatusChip";
import { IconBolt, IconPlus, IconSearch, IconServer } from "../ui/icons";
import styles from "./Dashboard.module.css";

export function Dashboard({
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
  const running = servers.filter((s) => s.state === "running").length;
  const live = servers.filter((s) =>
    ["starting", "stopping", "adopting"].includes(s.state),
  ).length;

  return (
    <div className={styles.page}>
      <header className={styles.pageHead}>
        <div>
          <h1 className={styles.title}>Overview</h1>
          <p className={styles.subtitle}>
            {status === "ready"
              ? daemon
                ? `${daemon.name} v${daemon.version} is online — ${running} of ${servers.length} running`
                : "Daemon is online"
              : "Waiting for the daemon…"}
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
          <div className={styles.grid}>
            {servers.map((server) => (
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
                  <span className={styles.cardOpen}>Open workspace →</span>
                </div>
              </div>
            ))}
          </div>
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
