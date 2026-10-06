// Panel shell (P3-1): sidebar server list + connection badge. The full
// vertical slice (detail view, terminal, palette, crash card) builds on
// this layout in the next slice.

import { useEffect, useMemo } from "react";
import { useConnection } from "../state/connection";
import { sortedServers, useServers } from "../state/servers";
import { startWire } from "../state/wire";
import { StatusDot } from "../ui/StatusDot";
import styles from "./App.module.css";
import listStyles from "./ServerList.module.css";

export function App() {
  useEffect(() => {
    startWire();
  }, []);

  const status = useConnection((s) => s.status);
  const daemon = useConnection((s) => s.daemon);
  const lastError = useConnection((s) => s.lastError);
  const serverMap = useServers((s) => s.servers);
  const servers = useMemo(() => sortedServers(serverMap), [serverMap]);

  return (
    <div className={styles.shell}>
      <header className={styles.topbar}>
        <div className={styles.brand}>
          ZaminPanel
          <span className={styles.brandSub}>
            {daemon ? `${daemon.name} v${daemon.version}` : "\u00a0"}
          </span>
        </div>
        <ConnectionBadge status={status} detail={lastError} />
      </header>

      <nav className={styles.sidebar} aria-label="Servers">
        <ServerList servers={servers} />
      </nav>

      <main className={styles.content}>
        {servers.length === 0 && status === "ready" ? (
          <p className={listStyles.empty}>
            No servers registered yet. Register one with
            <code> zamin register &lt;id&gt; &lt;path&gt;</code>, or wait for the next
            slice: the New Server flow lives here.
          </p>
        ) : null}
      </main>
    </div>
  );
}

function ConnectionBadge({ status, detail }: { status: string; detail: string | null }) {
  const tone =
    status === "ready" ? "var(--success)" : status === "connecting" ? "var(--warning)" : "var(--danger)";
  const label = status === "ready" ? "daemon online" : status === "connecting" ? "connecting…" : "daemon offline";
  return (
    <span
      title={detail ?? undefined}
      style={{ display: "inline-flex", gap: "var(--space-2)", alignItems: "center", color: "var(--text-muted)" }}
    >
      <span style={{ width: 8, height: 8, borderRadius: "50%", background: tone, display: "inline-block" }} />
      {label}
    </span>
  );
}

function ServerList({ servers }: { servers: ReturnType<typeof sortedServers> }) {
  if (servers.length === 0) return null;
  return (
    <ul className={listStyles.list}>
      {servers.map((server) => (
        <li key={server.serverId} className={listStyles.item}>
          <StatusDot state={server.state} />
          <span className={listStyles.name}>{server.displayName}</span>
        </li>
      ))}
    </ul>
  );
}
