// Panel shell: a proper control-center frame. The left rail carries the
// brand, primary navigation (Overview, the server fleet, and the reserved
// "Model" slot for a future capability), and the daemon footer; the main
// area is either the Overview dashboard or one server's workspace.
// Ctrl/Cmd+K remains the keyboard surface for the §53 journey.

import { Suspense, lazy, useEffect, useMemo } from "react";
import { useConnection } from "../state/connection";
import { activeProfile, useConnections } from "../state/connections";
import { useServers } from "../state/servers";
import { useUi } from "../state/ui";
import { startWire } from "../state/wire";
import { Button } from "../ui/Button";
import { StatusDot } from "../ui/StatusDot";
import { IconDashboard, IconPlus, IconServer, IconSparkles } from "../ui/icons";
import { Dashboard } from "./Dashboard";
import { ServerView } from "./ServerView";
import styles from "./App.module.css";
import listStyles from "./ServerList.module.css";

// Modals are operator-invoked overlays, not boot surfaces: each loads on
// first open so the cold start ships only the shell + dashboard
// (PERFORMANCE-BUDGETS: cold start → interactive).
const ConnectionsModal = lazy(() =>
  import("./ConnectionsModal").then((m) => ({ default: m.ConnectionsModal })),
);
const NewServerModal = lazy(() =>
  import("./NewServerModal").then((m) => ({ default: m.NewServerModal })),
);
const Palette = lazy(() => import("./Palette").then((m) => ({ default: m.Palette })));

export function App() {
  useEffect(() => {
    startWire();
  }, []);

  const status = useConnection((s) => s.status);
  const daemon = useConnection((s) => s.daemon);
  const lastError = useConnection((s) => s.lastError);
  const serverMap = useServers((s) => s.servers);
  const servers = useMemo(() => Object.values(serverMap), [serverMap]);
  const activeTab = useUi((s) => s.activeTab);
  const openServer = useUi((s) => s.openServer);
  const setActive = useUi((s) => s.setActive);
  const newServerOpen = useUi((s) => s.newServerOpen);
  const setNewServerOpen = useUi((s) => s.setNewServerOpen);
  const paletteOpen = useUi((s) => s.paletteOpen);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  const setConnectionsOpen = useUi((s) => s.setConnectionsOpen);
  const activeConnection = useConnections((s) => activeProfile(s));

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen(!useUi.getState().paletteOpen);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setPaletteOpen]);

  const sorted = [...servers].sort((a, b) => a.displayName.localeCompare(b.displayName));
  const activeServer = activeTab ? serverMap[activeTab] : undefined;
  const statusLabel =
    status === "ready" ? "daemon online" : status === "connecting" ? "connecting…" : "daemon offline";
  const statusColor =
    status === "ready" ? "var(--success)" : status === "connecting" ? "var(--warning)" : "var(--danger)";

  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar}>
        <div className={styles.brand}>
          <span className={styles.logoMark} aria-hidden>
            <svg viewBox="0 0 32 32" width="26" height="26">
              <rect x="1.5" y="1.5" width="29" height="29" rx="8" fill="var(--accent-soft)" stroke="var(--accent-border)" />
              <path d="M9 9h14v4.4h-9.2v2.4H21v4.4h-7.2v2.4H23V27H9V9z" fill="var(--accent)" opacity="0" />
              <path d="M10 10h12v3.6h-8.4v2.2H20v3.6h-6.4v2.2H22V25H10V10z" fill="var(--accent)" />
              <circle cx="21.5" cy="11.8" r="1.7" fill="var(--success)" />
            </svg>
          </span>
          <span className={styles.brandName}>ZaminPanel</span>
        </div>

        <nav className={styles.nav} aria-label="Primary">
          <button
            className={`${styles.navItem} ${activeTab === null ? styles.navItemActive : ""}`}
            onClick={() => setActive(null)}
            aria-label="Overview"
          >
            <IconDashboard />
            <span>Overview</span>
          </button>

          <div className={styles.section}>
            <span className={styles.sectionLabel}>Servers</span>
            <Button variant="primary" onClick={() => setNewServerOpen(true)}>
              <IconPlus size={14} />
              New server
            </Button>
            {sorted.length > 0 ? (
              <ul className={listStyles.list}>
                {sorted.map((server) => (
                  <li
                    key={server.serverId}
                    className={[
                      listStyles.item,
                      server.serverId === activeTab ? listStyles.itemActive : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    onClick={() => openServer(server.serverId)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") openServer(server.serverId);
                    }}
                    tabIndex={0}
                    role="button"
                    aria-label={`Open ${server.displayName}`}
                  >
                    <StatusDot state={server.state} />
                    <span className={listStyles.name}>{server.displayName}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className={styles.sideHint}>
                <IconServer size={14} />
                Nothing registered yet
              </p>
            )}
          </div>

          <div className={styles.section}>
            <span className={styles.sectionLabel}>System</span>
            <div
              className={`${styles.navItem} ${styles.navItemReserved}`}
              aria-disabled="true"
              title="Planned — the Model workspace lands here"
            >
              <IconSparkles />
              <span>Model</span>
              <span className={styles.soonChip}>soon</span>
            </div>
          </div>
        </nav>

        <footer className={styles.sideFoot}>
          <div
            className={styles.daemonRow}
            title={lastError ?? undefined}
          >
            <span
              className={styles.daemonDot}
              style={{ background: statusColor }}
              aria-hidden
            />
            <span className={styles.daemonLabel}>{statusLabel}</span>
          </div>
          <div className={styles.daemonMeta}>
            <span className={styles.daemonVersion}>
              {daemon ? `${daemon.name} v${daemon.version}` : "zamind"}
            </span>
            <button
              className={styles.kbdHint}
              onClick={() => setConnectionsOpen(true)}
              aria-label="Switch connection"
              title={`Connection: ${activeConnection.name}`}
            >
              {activeConnection.id === "local" ? "Local" : "Remote"}
            </button>
            <button
              className={styles.kbdHint}
              onClick={() => setPaletteOpen(true)}
              aria-label="Open command palette"
              title="Command palette"
            >
              Ctrl K
            </button>
          </div>
        </footer>
      </aside>

      <main className={styles.main}>
        {activeServer ? (
          <ServerView serverId={activeServer.serverId} />
        ) : (
          <Dashboard servers={sorted} onOpen={openServer} onNewServer={() => setNewServerOpen(true)} />
        )}
      </main>

      <Suspense fallback={null}>
        {newServerOpen ? <NewServerModal /> : null}
        {paletteOpen ? <Palette /> : null}
        <ConnectionsModal />
      </Suspense>
    </div>
  );
}
