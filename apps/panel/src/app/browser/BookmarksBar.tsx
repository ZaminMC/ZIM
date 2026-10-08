// The bookmark bar (§55, ADR-0023): the tool bar's second row, behaving
// like a browser's. A chip IS a typed destination — clicking navigates
// the active tab (and §61 singleton focus decides whether that means
// travel or focus, inherited from the tabs store, not re-decided here);
// Ctrl/Cmd+click or middle-click opens a NEW tab, the browser
// convention. The context menu carries the identity operations:
// open-in-new-tab, rename, remove. The bar is panel-local, shared
// across windows — the tabs differ per window, the bar does not.

import { useEffect, useRef, useState } from "react";
import { useBookmarks } from "../../state/bookmarks";
import { useTabs } from "../../state/tabs";
import { IconDashboard, IconGear, IconOpenInNew, IconServer, IconTerminal } from "../../ui/icons";
import styles from "./BookmarksBar.module.css";

function ChipIcon({ kind }: { kind: string }) {
  switch (kind) {
    case "server":
      return <IconServer size={12} />;
    case "console":
      return <IconTerminal size={12} />;
    case "settings":
      return <IconGear size={12} />;
    case "servers":
      return <IconDashboard size={12} />;
    default:
      return null;
  }
}

export function BookmarksBar() {
  const items = useBookmarks((s) => s.items);
  const barVisible = useBookmarks((s) => s.barVisible);
  const remove = useBookmarks((s) => s.remove);
  const rename = useBookmarks((s) => s.rename);

  const [menuFor, setMenuFor] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (menuFor === null) return;
    const dismiss = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuFor(null);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMenuFor(null);
    };
    window.addEventListener("mousedown", dismiss);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", dismiss);
      window.removeEventListener("keydown", onKey);
    };
  }, [menuFor]);

  if (!barVisible || items.length === 0) return null;

  const open = (id: string) => {
    const item = items.find((b) => b.id === id);
    if (!item) return;
    useTabs.getState().navigate(item.destination);
  };
  const openNew = (id: string) => {
    const item = items.find((b) => b.id === id);
    if (!item) return;
    useTabs.getState().openInNewTab(item.destination);
  };

  return (
    <div className={styles.bar} role="toolbar" aria-label="Bookmarks">
      {items.map((item) => (
        <div key={item.id} className={styles.itemWrap}>
          <button
            className={styles.chip}
            title={item.label}
            onClick={(event) => {
              if (event.ctrlKey || event.metaKey || event.button === 1) openNew(item.id);
              else open(item.id);
            }}
            onAuxClick={(event) => {
              // Middle-click: the browser's own new-tab gesture.
              if (event.button === 1) {
                event.preventDefault();
                openNew(item.id);
              }
            }}
            onContextMenu={(event) => {
              event.preventDefault();
              setMenuFor(item.id);
            }}
          >
            <ChipIcon kind={item.destination.kind} />
            <span className={styles.label}>{item.label}</span>
          </button>
          {menuFor === item.id ? (
            <div className={styles.menu} role="menu" ref={menuRef}>
              <button
                role="menuitem"
                className={styles.menuItem}
                onClick={() => {
                  setMenuFor(null);
                  openNew(item.id);
                }}
              >
                <IconOpenInNew size={12} /> Open in new tab
              </button>
              <button
                role="menuitem"
                className={styles.menuItem}
                onClick={() => {
                  setMenuFor(null);
                  const label = window.prompt("Rename bookmark", item.label);
                  if (label !== null) rename(item.id, label);
                }}
              >
                Rename…
              </button>
              <button
                role="menuitem"
                className={styles.menuItem}
                onClick={() => {
                  setMenuFor(null);
                  remove(item.id);
                }}
              >
                Remove
              </button>
            </div>
          ) : null}
        </div>
      ))}
    </div>
  );
}
