// Panel shell: sidebar (server list + New server), tab strip, content area,
// command palette, and modals. Ctrl/Cmd+K is the keyboard surface for the
// §53 journey.

import { useEffect, useMemo } from "react";
import { useConnection } from "../state/connection";
import { useServers } from "../state/servers";
import { useUi } from "../state/ui";
import { startWire } from "../state/wire";
import { Button } from "../ui/Button";
import { StatusDot } from "../ui/StatusDot";
import { NewServerModal } from "./NewServerModal";
import { Palette } from "./Palette";
import { ServerView } from "./ServerView";
import { TabStrip } from "./TabStrip";
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
  const servers = useMemo(() => Object.values(serverMap), [serverMap]);
  const activeTab = useUi((s) => s.activeTab);
  const openServer = useUi((s) => s.openServer);
  const newServerOpen = useUi((s) => s.newServerOpen);
  const setNewServerOpen = useUi((s) => s.setNewServerOpen);
  const paletteOpen = useUi((s) => s.paletteOpen);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);

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

  return (
    <div className={styles.shell}>
      <header className={styles.topbar}>
        <div className={styles.brand}>
          ZaminPanel
          <span className={styles.brandSub}>
            {daemon ? `${daemon.name} v${daemon.version}` : "\u00a0"}
          </span>
        </div>
        <span className={styles.badge} title={lastError ?? undefined}>
          <span
            className={styles.badgeDot}
            style={{
              background:
                status === "ready"
                  ? "var(--success)"
                  : status === "connecting"
                    ? "var(--warning)"
                    : "var(--danger)",
            }}
          />
          {status === "ready" ? "daemon online" : status === "connecting" ? "connecting…" : "daemon offline"}
        </span>
      </header>

      <nav className={styles.sidebar} aria-label="Servers">
        <div className={styles.sidebarHead}>
          <span>Servers</span>
        </div>
        <Button variant="primary" onClick={() => setNewServerOpen(true)}>
          + New server
        </Button>
        {sorted.length > 0 ? (
          <ul className={listStyles.list}>
            {sorted.map((server) => (
              <li
                key={server.serverId}
                className={listStyles.item}
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
        ) : null}
      </nav>

      <div className={styles.main}>
        <TabStrip />
        <main className={styles.content}>
          {activeServer ? (
            <ServerView serverId={activeServer.serverId} />
          ) : (
            <p className={listStyles.empty}>
              {servers.length === 0
                ? "No servers yet. Use “+ New server” (or the Ctrl+K palette) to register an existing server directory."
                : "Pick a server from the sidebar, or press Ctrl+K for commands."}
            </p>
          )}
        </main>
      </div>

      {newServerOpen ? <NewServerModal /> : null}
      {paletteOpen ? <Palette /> : null}
    </div>
  );
}
