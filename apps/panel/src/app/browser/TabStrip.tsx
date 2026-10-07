// The tab strip (ADR-0015): every tab is a typed destination; a server tab
// wears its state as the favicon wears a color. Right-click carries the
// §48 core verbs (reload, close, close others, close to the right); the
// founder's remaining menu items are reserved rooms, not fake entries.

import { useEffect, useState } from "react";
import { useServers, type ServerEntry } from "../../state/servers";
import { tabDestination, tabKeyOf, useTabs, type Tab } from "../../state/tabs";
import { Button } from "../../ui/Button";
import { StatusDot } from "../../ui/StatusDot";
import {
  IconBolt,
  IconClose,
  IconDashboard,
  IconGear,
  IconPlus,
  IconSearch,
  IconServer,
} from "../../ui/icons";
import styles from "./TabStrip.module.css";

function tabTitle(tab: Tab, entries: ServerEntry[]): string {
  const dest = tabDestination(tab);
  switch (dest.kind) {
    case "servers":
      return "Servers";
    case "new":
      return "New tab";
    case "settings":
      return "Settings";
    case "server":
      return entries.find((e) => e.serverId === dest.serverId)?.displayName ?? dest.serverId;
    case "missing":
      return dest.url;
  }
}

function TabIcon({ tab, entries }: { tab: Tab; entries: ServerEntry[] }) {
  const dest = tabDestination(tab);
  switch (dest.kind) {
    case "servers":
      return <IconDashboard size={13} />;
    case "new":
      return <IconSearch size={13} />;
    case "settings":
      return <IconGear size={13} />;
    case "server": {
      const entry = entries.find((e) => e.serverId === dest.serverId);
      return entry ? <StatusDot state={entry.state} /> : <IconServer size={13} />;
    }
    case "missing":
      return <IconBolt size={13} />;
  }
}

interface TabMenu {
  key: string;
  x: number;
  y: number;
}

export function TabStrip() {
  const tabs = useTabs((s) => s.tabs);
  const activeKey = useTabs((s) => s.activeKey);
  const setActive = useTabs((s) => s.setActive);
  const close = useTabs((s) => s.close);
  const closeOthers = useTabs((s) => s.closeOthers);
  const closeToTheRight = useTabs((s) => s.closeToTheRight);
  const newTab = useTabs((s) => s.newTab);
  const serverMap = useServers((s) => s.servers);
  const entries = Object.values(serverMap);
  const [menu, setMenu] = useState<TabMenu | null>(null);

  // The context menu dismisses on any click outside itself or on Escape.
  useEffect(() => {
    if (!menu) return;
    const dismiss = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dismiss();
    };
    window.addEventListener("mousedown", dismiss);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", dismiss);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  return (
    <div className={styles.strip}>
      <div className={styles.tabs} role="tablist" aria-label="Open tabs">
        {tabs.map((tab) => {
          const key = tabKeyOf(tab);
          const active = key === activeKey;
          return (
            <div
              key={key}
              role="tab"
              aria-selected={active}
              tabIndex={0}
              title={tabTitle(tab, entries)}
              className={`${styles.tab} ${active ? styles.tabActive : ""}`}
              onClick={() => setActive(key)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  setActive(key);
                }
              }}
              onAuxClick={(e) => {
                if (e.button === 1) {
                  e.preventDefault();
                  close(key);
                }
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ key, x: e.clientX, y: e.clientY });
              }}
            >
              <span className={styles.icon} aria-hidden>
                <TabIcon tab={tab} entries={entries} />
              </span>
              <span className={styles.title}>{tabTitle(tab, entries)}</span>
              <button
                className={styles.close}
                aria-label={`Close ${tabTitle(tab, entries)}`}
                title="Close tab"
                onClick={(e) => {
                  e.stopPropagation();
                  close(key);
                }}
              >
                <IconClose size={11} />
              </button>
            </div>
          );
        })}
      </div>
      <Button
        variant="ghost"
        className={styles.newButton}
        onClick={() => newTab()}
        aria-label="New tab (Ctrl+T)"
        title="New tab (Ctrl+T)"
      >
        <IconPlus size={14} />
      </Button>
      {menu ? (
        <div
          className={styles.menu}
          style={{ left: menu.x, top: menu.y }}
          role="menu"
          // Keep the window-level dismiss (mousedown) from firing on the
          // menu's own opening click.
          onMouseDown={(e) => e.stopPropagation()}
        >
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              setActive(menu.key);
              useTabs.getState().reload();
              setMenu(null);
            }}
          >
            Reload
          </button>
          <div className={styles.divider} />
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              close(menu.key);
              setMenu(null);
            }}
          >
            Close
          </button>
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              closeOthers(menu.key);
              setMenu(null);
            }}
          >
            Close other tabs
          </button>
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              closeToTheRight(menu.key);
              setMenu(null);
            }}
          >
            Close tabs to the right
          </button>
          <div className={styles.divider} />
          <button
            role="menuitem"
            className={`${styles.menuItem} ${styles.menuItemReserved}`}
            disabled
            title="Planned — tab groups land with their real machinery"
          >
            Add tab to new group…
          </button>
          <button
            role="menuitem"
            className={`${styles.menuItem} ${styles.menuItemReserved}`}
            disabled
            title="Planned — move to window lands with its real machinery"
          >
            Move tab to new window
          </button>
        </div>
      ) : null}
    </div>
  );
}
