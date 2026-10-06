// Open-server tabs. View state (open, order, active) is Panel-local
// (ADR-0003) — persisted by the ui store, never daemon state.

import { useServers } from "../state/servers";
import { useUi } from "../state/ui";
import { StatusDot } from "../ui/StatusDot";
import styles from "./TabStrip.module.css";

export function TabStrip() {
  const openTabs = useUi((s) => s.openTabs);
  const activeTab = useUi((s) => s.activeTab);
  const setActive = useUi((s) => s.setActive);
  const closeTab = useUi((s) => s.closeTab);
  const serverMap = useServers((s) => s.servers);

  if (openTabs.length === 0) return null;

  return (
    <div className={styles.tabs} role="tablist" aria-label="Open servers">
      {openTabs.map((serverId) => {
        const server = serverMap[serverId];
        const active = serverId === activeTab;
        return (
          <div
            key={serverId}
            role="tab"
            aria-selected={active}
            tabIndex={0}
            className={[styles.tab, active ? styles.tabActive : ""].filter(Boolean).join(" ")}
            onClick={() => setActive(serverId)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") setActive(serverId);
            }}
          >
            {server ? <StatusDot state={server.state} /> : null}
            <span>{server?.displayName ?? serverId}</span>
            <button
              className={styles.close}
              aria-label={`Close ${server?.displayName ?? serverId}`}
              onClick={(event) => {
                event.stopPropagation();
                closeTab(serverId);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
    </div>
  );
}
