// The tab strip (ADR-0015, ADR-0016): every tab is a typed destination; a
// server tab wears its state as the favicon wears a color. Pinned tabs
// (§52) sit compact at the head, guarded from accidental closure. Groups
// (§49) render as collapsible browser-style chips — never folders. The
// context menu carries the §48 verbs that have real machinery; the
// founder's remaining entries stay honest reserved rooms, not fakes.

import { useEffect, useState } from "react";
import { useServers, type ServerEntry } from "../../state/servers";
import {
  tabDestination,
  useTabs,
  type GroupId,
  type Tab,
  type TabId,
} from "../../state/tabs";
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
  id: TabId;
  x: number;
  y: number;
}

export function TabStrip() {
  const tabs = useTabs((s) => s.tabs);
  const activeId = useTabs((s) => s.activeId);
  const groups = useTabs((s) => s.groups);
  const recentlyClosed = useTabs((s) => s.recentlyClosed);
  const setActive = useTabs((s) => s.setActive);
  const close = useTabs((s) => s.close);
  const closeOthers = useTabs((s) => s.closeOthers);
  const closeToTheRight = useTabs((s) => s.closeToTheRight);
  const newTab = useTabs((s) => s.newTab);
  const newTabToTheRight = useTabs((s) => s.newTabToTheRight);
  const duplicate = useTabs((s) => s.duplicate);
  const togglePin = useTabs((s) => s.togglePin);
  const reopen = useTabs((s) => s.reopen);
  const addToNewGroup = useTabs((s) => s.addToNewGroup);
  const moveToGroup = useTabs((s) => s.moveToGroup);
  const removeFromGroup = useTabs((s) => s.removeFromGroup);
  const toggleGroupCollapse = useTabs((s) => s.toggleGroupCollapse);
  const renameGroup = useTabs((s) => s.renameGroup);
  const serverMap = useServers((s) => s.servers);
  const entries = Object.values(serverMap);
  const [menu, setMenu] = useState<TabMenu | null>(null);
  const [moveOpen, setMoveOpen] = useState(false);
  const [renaming, setRenaming] = useState<GroupId | null>(null);
  const [renameDraft, setRenameDraft] = useState("");

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

  const openMenu = (id: TabId, x: number, y: number) => {
    setMenu({ id, x, y });
    setMoveOpen(false);
  };

  const commitRename = () => {
    if (renaming) renameGroup(renaming, renameDraft);
    setRenaming(null);
  };

  const menuTab = menu ? tabs.find((t) => t.id === menu.id) : undefined;
  const groupIds = Object.keys(groups);

  return (
    <div className={styles.strip}>
      <div className={styles.tabs} role="tablist" aria-label="Open tabs">
        {tabs.map((tab, index) => {
          const active = tab.id === activeId;
          const group = tab.groupId ? groups[tab.groupId] : undefined;
          const prev = index > 0 ? tabs[index - 1] : undefined;
          const chipFor =
            tab.groupId !== undefined && prev?.groupId !== tab.groupId ? tab.groupId : null;
          const collapsed = group?.collapsed ?? false;
          const memberCount = tab.groupId
            ? tabs.filter((t) => t.groupId === tab.groupId).length
            : 0;

          const chip =
            chipFor && group ? (
              renaming === chipFor ? (
                <input
                  className={styles.renameInput}
                  value={renameDraft}
                  autoFocus
                  aria-label="Group name"
                  onChange={(e) => setRenameDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commitRename();
                    if (e.key === "Escape") setRenaming(null);
                  }}
                  onBlur={commitRename}
                  onClick={(e) => e.stopPropagation()}
                />
              ) : (
                <button
                  type="button"
                  className={`${styles.groupChip} ${styles[`group_${group.color}`]}`}
                  aria-expanded={!collapsed}
                  title={`${group.label} — click to ${collapsed ? "expand" : "collapse"}, double-click to rename`}
                  onClick={() => toggleGroupCollapse(chipFor)}
                  onDoubleClick={(e) => {
                    e.stopPropagation();
                    setRenameDraft(group.label);
                    setRenaming(chipFor);
                  }}
                >
                  <span className={styles.groupChevron} aria-hidden>
                    {collapsed ? "▸" : "▾"}
                  </span>
                  <span className={styles.groupLabel}>{group.label}</span>
                  {collapsed ? <span className={styles.groupCount}>{memberCount}</span> : null}
                </button>
              )
            ) : null;

          // §49: a collapsed group shows its chip (the first member's slot)
          // and hides every member tab.
          if (collapsed && tab.groupId) {
            return chip ? (
              <div key={tab.id} className={styles.groupWrap}>
                {chip}
              </div>
            ) : null;
          }

          return (
            <div key={tab.id} className={styles.groupWrap}>
              {chip}
              <div
                role="tab"
                aria-selected={active}
                tabIndex={0}
                title={tabTitle(tab, entries)}
                className={`${styles.tab} ${active ? styles.tabActive : ""} ${
                  tab.pinned ? styles.tabPinned : ""
                } ${group ? `${styles.groupMember} ${styles[`group_${group.color}`]}` : ""}`}
                onClick={() => setActive(tab.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    setActive(tab.id);
                  }
                }}
                onAuxClick={(e) => {
                  if (e.button === 1 && !tab.pinned) {
                    // §52: the middle click is the accidental close; a
                    // pinned tab ignores it. The menu's Close stays explicit.
                    e.preventDefault();
                    close(tab.id);
                  }
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  openMenu(tab.id, e.clientX, e.clientY);
                }}
              >
                <span className={styles.icon} aria-hidden>
                  <TabIcon tab={tab} entries={entries} />
                </span>
                {!tab.pinned ? (
                  <>
                    <span className={styles.title}>{tabTitle(tab, entries)}</span>
                    <button
                      className={styles.close}
                      aria-label={`Close ${tabTitle(tab, entries)}`}
                      title="Close tab"
                      onClick={(e) => {
                        e.stopPropagation();
                        close(tab.id);
                      }}
                    >
                      <IconClose size={11} />
                    </button>
                  </>
                ) : null}
              </div>
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
      {menu && menuTab ? (
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
              newTabToTheRight(menu.id);
              setMenu(null);
            }}
          >
            New tab to the right
          </button>
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              setActive(menu.id);
              useTabs.getState().reload();
              setMenu(null);
            }}
          >
            Reload
          </button>
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              duplicate(menu.id);
              setMenu(null);
            }}
          >
            Duplicate
          </button>
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              togglePin(menu.id);
              setMenu(null);
            }}
          >
            {menuTab.pinned ? "Unpin" : "Pin"}
          </button>
          <button
            role="menuitem"
            className={`${styles.menuItem} ${styles.menuItemReserved}`}
            disabled
            title="Planned — audio controls land with their real machinery"
          >
            Mute
          </button>
          <div className={styles.divider} />
          {menuTab.pinned ? (
            <button
              role="menuitem"
              className={`${styles.menuItem} ${styles.menuItemReserved}`}
              disabled
              title="Pinned tabs stay out of groups (§52)"
            >
              Add tab to new group…
            </button>
          ) : (
            <>
              <button
                role="menuitem"
                className={styles.menuItem}
                onClick={() => {
                  addToNewGroup(menu.id);
                  setMenu(null);
                }}
              >
                Add tab to new group…
              </button>
              {groupIds.length > 0 ? (
                <button
                  role="menuitem"
                  className={styles.menuItem}
                  aria-expanded={moveOpen}
                  onClick={() => setMoveOpen(!moveOpen)}
                >
                  Move to group {moveOpen ? "▾" : "▸"}
                </button>
              ) : null}
              {moveOpen
                ? groupIds.map((gid) => {
                    const g = groups[gid];
                    if (!g) return null;
                    return (
                      <button
                        key={gid}
                        role="menuitem"
                        className={`${styles.menuItem} ${styles.menuItemSub} ${
                          styles[`group_${g.color}`]
                        }`}
                        onClick={() => {
                          moveToGroup(menu.id, gid);
                          setMenu(null);
                        }}
                      >
                        {g.label}
                      </button>
                    );
                  })
                : null}
              {menuTab.groupId ? (
                <button
                  role="menuitem"
                  className={styles.menuItem}
                  onClick={() => {
                    removeFromGroup(menu.id);
                    setMenu(null);
                  }}
                >
                  Remove from group
                </button>
              ) : null}
            </>
          )}
          <button
            role="menuitem"
            className={`${styles.menuItem} ${styles.menuItemReserved}`}
            disabled
            title="Planned — move to window lands with its real machinery"
          >
            Move tab to new window
          </button>
          <div className={styles.divider} />
          <button
            role="menuitem"
            className={styles.menuItem}
            disabled={recentlyClosed.length === 0}
            title={
              recentlyClosed.length > 0
                ? "Ctrl+Shift+T"
                : "Nothing recently closed"
            }
            onClick={() => {
              reopen();
              setMenu(null);
            }}
          >
            Reopen closed tab
          </button>
          <button
            role="menuitem"
            className={`${styles.menuItem} ${styles.menuItemReserved}`}
            disabled
            title="Reserved for Dutchmen — the room stays, the machinery waits"
          >
            Share tab with Dutchmen
          </button>
          <div className={styles.divider} />
          <button
            role="menuitem"
            className={`${styles.menuItem} ${styles.menuItemReserved}`}
            disabled
            title="Planned — the vertical strip lands with its real machinery"
          >
            Show tabs vertically
          </button>
          <div className={styles.divider} />
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              close(menu.id);
              setMenu(null);
            }}
          >
            Close
          </button>
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              closeOthers(menu.id);
              setMenu(null);
            }}
          >
            Close other tabs
          </button>
          <button
            role="menuitem"
            className={styles.menuItem}
            onClick={() => {
              closeToTheRight(menu.id);
              setMenu(null);
            }}
          >
            Close tabs to the right
          </button>
        </div>
      ) : null}
    </div>
  );
}
