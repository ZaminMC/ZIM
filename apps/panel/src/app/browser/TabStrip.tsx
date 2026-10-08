// The tab strip (ADR-0015, ADR-0016, ADR-0018): every tab is a typed
// destination; a server tab wears its state as the favicon wears a color.
// Pinned tabs (§52) sit compact at the head, guarded from accidental
// closure. Groups (§49) render as collapsible browser-style chips — never
// folders. Tabs drag (§90's operator machinery): the strip shows the
// insertion edge, the pinned head clamps, and dragging out of a group
// leaves it. "Move tab to new window" (§50) is live machinery now: the
// store hands the tab over, the opener opens a second ZaminPanel window
// that claims it by hash, and a blocked popup brings the tab home. The
// founder's remaining entries stay honest reserved rooms, not fakes.

import { useEffect, useRef, useState } from "react";
import { useServers, type ServerEntry } from "../../state/servers";
import {
  HANDOFF_PREFIX,
  tabDestination,
  useTabs,
  type GroupId,
  type Tab,
  type TabId,
} from "../../state/tabs";
import { Button } from "../../ui/Button";
import { StatusDot } from "../../ui/StatusDot";
import {
  IconAudit,
  IconBolt,
  IconClose,
  IconDashboard,
  IconExtension,
  IconFeedback,
  IconGear,
  IconInfo,
  IconJobs,
  IconPlus,
  IconSearch,
  IconServer,
  IconTerminal,
  IconVolumeMuted,
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
    case "console": {
      const name = entries.find((e) => e.serverId === dest.serverId)?.displayName ?? dest.serverId;
      return `${name} console`;
    }
    case "jobs":
      return "Jobs";
    case "audit":
      return "Audit log";
    case "about":
      return "About ZaminPanel";
    case "feedback":
      return "Feedback";
    case "extensions":
      return "Extensions";
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
    case "console":
      return <IconTerminal size={13} />;
    case "jobs":
      return <IconJobs size={13} />;
    case "audit":
      return <IconAudit size={13} />;
    case "about":
      return <IconInfo size={13} />;
    case "feedback":
      return <IconFeedback size={13} />;
    case "extensions":
      return <IconExtension size={13} />;
    case "missing":
      return <IconBolt size={13} />;
  }
}

interface TabMenu {
  id: TabId;
  x: number;
  y: number;
}

/** Which side of a tab a drop would land on — the pointer against the
 *  tab's own midpoint, along the strip's axis (§54: a vertical rail
 *  splits above/below, a horizontal strip before/after). Used by
 *  dragover (the indicator) and drop (the math), so the two can never
 *  disagree. */
function sideOf(
  el: HTMLElement,
  clientX: number,
  clientY: number,
  vertical: boolean,
): "before" | "after" {
  const rect = el.getBoundingClientRect();
  if (vertical) return clientY < rect.top + rect.height / 2 ? "before" : "after";
  return clientX < rect.left + rect.width / 2 ? "before" : "after";
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
  const reorder = useTabs((s) => s.reorder);
  const moveToNewWindow = useTabs((s) => s.moveToNewWindow);
  const restoreMoved = useTabs((s) => s.restoreMoved);
  const toggleMute = useTabs((s) => s.toggleMute);
  const vertical = useTabs((s) => s.verticalStrip);
  const toggleVertical = useTabs((s) => s.toggleVerticalStrip);
  const serverMap = useServers((s) => s.servers);
  const entries = Object.values(serverMap);
  const [menu, setMenu] = useState<TabMenu | null>(null);
  const [moveOpen, setMoveOpen] = useState(false);
  const [renaming, setRenaming] = useState<GroupId | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  // The dragged tab id rides a ref, not state: dragover may not read the
  // payload, and a ref survives re-renders without re-rendering the strip
  // by itself. Only the insertion hint is state — it is visual.
  const dragIdRef = useRef<TabId | null>(null);
  const [dropHint, setDropHint] = useState<{ id: TabId; side: "before" | "after" } | null>(null);

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
    <div
      className={`${styles.strip} ${vertical ? styles.stripVertical : ""}`}
      aria-orientation={vertical ? "vertical" : "horizontal"}
    >
      <div
        className={`${styles.tabs} ${vertical ? styles.tabsVertical : ""}`}
        role="tablist"
        aria-label="Open tabs"
        aria-orientation={vertical ? "vertical" : "horizontal"}
      >
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
                } ${group ? `${styles.groupMember} ${styles[`group_${group.color}`]}` : ""} ${
                  dropHint?.id === tab.id
                    ? dropHint.side === "before"
                      ? vertical
                        ? styles.dropAbove
                        : styles.dropBefore
                      : vertical
                        ? styles.dropBelow
                        : styles.dropAfter
                    : ""
                }`}
                draggable
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
                onDragStart={(e) => {
                  // Firefox refuses a payload-less drag; the id also rides
                  // the ref because dragover cannot read the payload.
                  e.dataTransfer.setData("text/plain", tab.id);
                  e.dataTransfer.effectAllowed = "move";
                  dragIdRef.current = tab.id;
                }}
                onDragOver={(e) => {
                  const dragId = dragIdRef.current;
                  if (dragId === null || dragId === tab.id) return;
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "move";
                  const side = sideOf(e.currentTarget, e.clientX, e.clientY, vertical);
                  setDropHint((prev) =>
                    prev?.id === tab.id && prev.side === side ? prev : { id: tab.id, side },
                  );
                }}
                onDrop={(e) => {
                  e.preventDefault();
                  const dragId = dragIdRef.current;
                  const base = tabs.findIndex((t) => t.id === tab.id);
                  if (dragId !== null && dragId !== tab.id && base !== -1) {
                    const insertion =
                      sideOf(e.currentTarget, e.clientX, e.clientY, vertical) === "after"
                        ? base + 1
                        : base;
                    reorder(dragId, insertion);
                  }
                  dragIdRef.current = null;
                  setDropHint(null);
                }}
                onDragEnd={() => {
                  dragIdRef.current = null;
                  setDropHint(null);
                }}
              >
                <span className={styles.icon} aria-hidden>
                  <TabIcon tab={tab} entries={entries} />
                </span>
                {!tab.pinned ? (
                  <>
                    <span className={styles.title}>{tabTitle(tab, entries)}</span>
                    {tab.muted ? (
                      // §53: the tab shows its audio posture the moment it
                      // is set — the indicator IS the state, not a control.
                      <span
                        className={styles.mutedMark}
                        aria-label="Muted"
                        role="img"
                        title="This tab is muted"
                      >
                        <IconVolumeMuted size={12} />
                      </span>
                    ) : null}
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
        className={`${styles.newButton} ${vertical ? styles.newButtonVertical : ""}`}
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
            className={styles.menuItem}
            onClick={() => {
              toggleMute(menu.id);
              setMenu(null);
            }}
            title="The tab's audio posture — audio surfaces consult it before they make a sound"
          >
            {menuTab.muted ? "Unmute tab" : "Mute tab"}
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
            className={styles.menuItem}
            disabled={tabs.length <= 1}
            title={
              tabs.length <= 1
                ? "The last tab cannot move — this window keeps one tab"
                : "Opens this tab in a new ZaminPanel window"
            }
            onClick={() => {
              const id = menu.id;
              const index = tabs.findIndex((t) => t.id === id);
              const moved = index === -1 ? undefined : tabs[index];
              const wasActive = activeId === id;
              const handoff = moveToNewWindow(id);
              setMenu(null);
              if (handoff === null || !moved) return;
              const opened = window.open(
                `${window.location.pathname}#handoff=${handoff}`,
                "_blank",
              );
              if (opened === null) {
                // A blocked popup must not swallow the tab: the slot dies
                // unclaimed and the tab comes home to its old place.
                try {
                  localStorage.removeItem(`${HANDOFF_PREFIX}${handoff}`);
                } catch {
                  // Denied storage: the restore itself still stands.
                }
                restoreMoved(moved, index, wasActive);
              }
            }}
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
            aria-checked={vertical}
            className={styles.menuItem}
            onClick={() => {
              toggleVertical();
              setMenu(null);
            }}
            title="§54 — the same tabs, rendered as a rail on the left"
          >
            {vertical ? "Use horizontal strip" : "Show tabs vertically"}
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
