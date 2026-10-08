// The tool bar (ADR-0015): back, forward, reload — the browser verbs —
// the address bar, and the system cluster (daemon pulse, connection, the
// ⋮ menu). §5: compact; navigation never wastes vertical space.

import { useEffect, useRef, useState } from "react";
import { useConnection } from "../../state/connection";
import { useBookmarks } from "../../state/bookmarks";
import { activeProfile, useConnections } from "../../state/connections";
import { hostHint } from "../../state/destinations";
import { sortedServers, useServers } from "../../state/servers";
import { canBack, canForward, tabDestination, useTabs } from "../../state/tabs";
import { useUi } from "../../state/ui";
import { Button } from "../../ui/Button";
import { IconDots, IconPlus, IconReload, IconArrowLeft, IconArrowRight } from "../../ui/icons";
import { AddressBar } from "./AddressBar";
import styles from "./ToolBar.module.css";

export function ToolBar() {
  const tabs = useTabs((s) => s.tabs);
  const activeTabId = useTabs((s) => s.activeId);
  const back = useTabs((s) => s.back);
  const forward = useTabs((s) => s.forward);
  const reload = useTabs((s) => s.reload);
  const setNewServerOpen = useUi((s) => s.setNewServerOpen);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  const setConnectionsOpen = useUi((s) => s.setConnectionsOpen);
  const remotes = useConnections((s) => s.remotes);
  const activeId = useConnections((s) => s.activeId);
  const bookmarksBarVisible = useBookmarks((s) => s.barVisible);
  const toggleBookmarksBar = useBookmarks((s) => s.toggleBar);
  const status = useConnection((s) => s.status);
  const daemon = useConnection((s) => s.daemon);
  const lastError = useConnection((s) => s.lastError);
  const serverMap = useServers((s) => s.servers);

  const active = tabs.find((t) => t.id === activeTabId) ?? tabs[0];
  const destination = active ? tabDestination(active) : undefined;
  const entries = sortedServers(serverMap);
  const profile = activeProfile({ remotes, activeId });
  const host = hostHint(
    "addr" in profile ? { local: false, remoteAddr: profile.addr } : { local: true },
  );

  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const dismiss = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenuOpen(false);
    };
    window.addEventListener("mousedown", dismiss);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", dismiss);
      window.removeEventListener("keydown", onKey);
    };
  }, [menuOpen]);

  const statusLabel =
    status === "ready" ? "daemon online" : status === "connecting" ? "connecting…" : "daemon offline";
  const statusColor =
    status === "ready" ? "var(--success)" : status === "connecting" ? "var(--warning, #e0b341)" : "var(--danger)";

  return (
    <div className={styles.bar}>
      <div className={styles.nav}>
        <Button
          variant="ghost"
          className={styles.navButton}
          onClick={back}
          disabled={!active || !canBack(active)}
          aria-label="Back (Alt+Left)"
          title="Back (Alt+Left)"
        >
          <IconArrowLeft size={15} />
        </Button>
        <Button
          variant="ghost"
          className={styles.navButton}
          onClick={forward}
          disabled={!active || !canForward(active)}
          aria-label="Forward (Alt+Right)"
          title="Forward (Alt+Right)"
        >
          <IconArrowRight size={15} />
        </Button>
        <Button
          variant="ghost"
          className={styles.navButton}
          onClick={reload}
          disabled={!active}
          aria-label="Reload (Ctrl+R)"
          title="Reload (Ctrl+R) — rebuilds the view, never the server"
        >
          <IconReload size={14} />
        </Button>
      </div>

      {destination ? <AddressBar destination={destination} entries={entries} host={host} /> : null}

      <div
        className={styles.pulse}
        title={lastError ?? statusLabel}
        aria-label={statusLabel}
      >
        <span className={styles.pulseDot} style={{ background: statusColor }} aria-hidden />
      </div>

      <button
        className={styles.connection}
        onClick={() => setConnectionsOpen(true)}
        title={`Connection: ${profile.name}`}
      >
        {profile.id === "local" ? "Local" : "Remote"}
      </button>

      <div className={styles.menuWrap} ref={menuRef}>
        <Button
          variant="ghost"
          className={styles.navButton}
          onClick={() => setMenuOpen((v) => !v)}
          aria-label="Menu"
          aria-expanded={menuOpen}
          title="Menu"
        >
          <IconDots size={15} />
        </Button>
        {menuOpen ? (
          <div className={styles.menu} role="menu">
            <button
              role="menuitem"
              className={styles.menuItem}
              onClick={() => {
                setMenuOpen(false);
                setNewServerOpen(true);
              }}
            >
              <IconPlus size={13} />
              New server
            </button>
            <button
              role="menuitem"
              className={styles.menuItem}
              onClick={() => {
                setMenuOpen(false);
                setPaletteOpen(true);
              }}
            >
              Command palette <span className={styles.kbd}>Ctrl K</span>
            </button>
            <button
              role="menuitemcheckbox"
              aria-checked={bookmarksBarVisible}
              className={styles.menuItem}
              onClick={() => {
                setMenuOpen(false);
                toggleBookmarksBar();
              }}
            >
              Bookmarks bar <span className={styles.kbd}>Ctrl Shift B</span>
            </button>
            <button
              role="menuitem"
              className={styles.menuItem}
              onClick={() => {
                setMenuOpen(false);
                useTabs.getState().navigate({ kind: "settings" });
              }}
            >
              Settings
            </button>
            <button
              role="menuitem"
              className={styles.menuItem}
              onClick={() => {
                setMenuOpen(false);
                setConnectionsOpen(true);
              }}
            >
              Connections…
            </button>
            <div className={styles.divider} />
            <div className={styles.menuStatus}>
              <span className={styles.pulseDot} style={{ background: statusColor }} aria-hidden />
              <span>{statusLabel}</span>
              <span className={styles.menuVersion}>
                {daemon ? `${daemon.name} v${daemon.version}` : "zamind"}
              </span>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
