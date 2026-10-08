// The chrome layer (ADR-0033 Phase 1): the VIEW of the Rust shell model.
// This document is the window's chrome — tab strip, toolbar, omnibox,
// bookmarks bar, window controls — and nothing else. Content lives in its
// own webview per tab. Every metric and interaction here renders from the
// host's snapshot: the layout law runs in the model (shell/layout.rs);
// this layer is deliberately dumb geometry and pointer reporting.
//
// Ported behaviors (citations in shell/):
// - tab slots from the width law (tab_width_constraints.cc),
// - drag: start→move→drop into the ported TabDragController (15 DIP
//   detach magnetism; reorder by slot centers),
// - commands: Chromium's ID space (chrome_command_ids.h) via one dispatch,
// - omnibox: the model classifies on every keystroke (autocomplete_input).

import { useCallback, useEffect, useRef, useState } from "react";
import {
  bootChrome,
  isTauri,
  omniboxClassify,
  omniboxCommit,
  onSnapshot,
  onFocusAddress,
  reportChromeSize,
  shellCommand,
  shellDrag,
  type Snapshot,
} from "./chromeIpc";
import { handleBrowserKey, type BrowserKeyApi } from "../state/browserKeys";
import "./chrome.css";

const CMD = {
  RELOAD: 33002,
  NEW_TAB: 34014,
  CLOSE_TAB: 34015,
  SELECT_NEXT_TAB: 34016,
  SELECT_PREVIOUS_TAB: 34017,
  SELECT_TAB_0: 34018,
  DUPLICATE_TAB: 34027,
  RESTORE_TAB: 34028,
  ADD_NEW_TAB_TO_GROUP: 34100,
  CLOSE_TAB_GROUP: 34104,
  BOOKMARK_THIS_TAB: 35000,
  FOCUS_LOCATION: 39001,
  SHOW_BOOKMARK_BAR: 40009,
  TOGGLE_PINNED: 50001,
  SELECT_LAST_TAB: 50002,
  TOGGLE_MUTE: 50003,
  NAVIGATE_ACTIVE: 50004,
  TOGGLE_GROUP_COLLAPSE: 50005,
  NAV_BACK: 50006,
  NAV_FORWARD: 50007,
  WINDOW_MINIMIZE: 50010,
  WINDOW_TOGGLE_MAXIMIZE: 50011,
  WINDOW_CLOSE: 50012,
} as const;

const GROUP_COLOR_VARS = [
  "var(--group-sky)",
  "var(--group-grass)",
  "var(--group-amber)",
  "var(--group-rose)",
  "var(--group-violet)",
  "var(--group-slate)",
];

export function ChromeApp() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const stripRef = useRef<HTMLDivElement | null>(null);
  const omniboxRef = useRef<HTMLInputElement | null>(null);
  const dragRef = useRef<{ tab: number; start_x: number; start_y: number; moved: boolean } | null>(null);
  const [menu, setMenu] = useState<{ tab: number; x: number; y: number } | null>(null);
  const [omniboxText, setOmniboxText] = useState<string | null>(null);
  const [joinNote, setJoinNote] = useState<string | null>(null);
  const [updateAvailable, setUpdateAvailable] = useState<string | null>(null);

  const selectSlotIndex = useCallback((slotIndex: number) => {
    void shellCommand(CMD.SELECT_TAB_0 + slotIndex);
  }, []);

  // Boot + the snapshot lane (the model pushes every change).
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void bootChrome().then((boot) => {
      if (!disposed && boot) setSnap(boot);
    });
    void onSnapshot((next) => {
      if (!disposed) setSnap(next);
    }).then((off) => {
      if (disposed) off();
      else unlisten = off;
    });
    // The model needs the chrome's true width for the layout law.
    const report = () => {
      if (stripRef.current) {
        const rect = stripRef.current.getBoundingClientRect();
        void reportChromeSize(rect.width, rect.height);
      }
    };
    report();
    window.addEventListener("resize", report);
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("resize", report);
    };
  }, []);

  // FOCUS_LOCATION → focus the omnibox; its resting text rides snapshots.
  useEffect(() => {
    return () => {
      void onFocusAddress(() => omniboxRef.current?.focus()).then((off) => off());
    };
  }, []);
  useEffect(() => {
    const offPromise = onFocusAddress(() => omniboxRef.current?.focus());
    return () => {
      void offPromise.then((off) => off());
    };
  }, []);

  // The omnibox rests on the model's address until the user edits it.
  useEffect(() => {
    setOmniboxText(null);
    setJoinNote(null);
  }, [snap?.address, snap?.active]);

  // The update lane (ADR-0024): the chrome owns the pill now.
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    const check = async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const update = await check();
        if (!cancelled && update) setUpdateAvailable(update.version ?? "");
      } catch {
        // The dev channel may be unreachable; the pill simply stays off.
      }
    };
    void check();
    const timer = window.setInterval(check, 6 * 60 * 60 * 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

  const installUpdate = useCallback(async () => {
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const { relaunch } = await import("@tauri-apps/plugin-process");
      const update = await check();
      if (update) {
        await update.install();
        await relaunch();
      }
    } catch {
      setUpdateAvailable(null);
    }
  }, []);

  // The browser keyboard contract (ADR-0032), chrome side.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as (HTMLElement & { isContentEditable: boolean }) | null;
      const api: BrowserKeyApi = {
        newTab: () => void shellCommand(CMD.NEW_TAB),
        reopenClosedTab: () => void shellCommand(CMD.RESTORE_TAB),
        closeActiveTab: () => void shellCommand(CMD.CLOSE_TAB),
        cycleTab: (step) =>
          void shellCommand(step >= 0 ? CMD.SELECT_NEXT_TAB : CMD.SELECT_PREVIOUS_TAB),
        selectTabIndex: (index) =>
          void shellCommand(
            index === "last" ? CMD.SELECT_LAST_TAB : CMD.SELECT_TAB_0 + index,
          ),
        focusAddressBar: () => omniboxRef.current?.focus(),
        reload: () => void shellCommand(CMD.RELOAD),
        goBack: () => void shellCommand(CMD.NAV_BACK),
        goForward: () => void shellCommand(CMD.NAV_FORWARD),
        bookmarkActive: () => void shellCommand(CMD.BOOKMARK_THIS_TAB),
        toggleBookmarksBar: () => void shellCommand(CMD.SHOW_BOOKMARK_BAR),
        togglePalette: () => {
          window.dispatchEvent(new CustomEvent("zamin:toggle-palette"));
        },
        paletteOpen: () => false,
      };
      const verdict = handleBrowserKey(
        event,
        {
          isContentEditable: target?.isContentEditable ?? false,
          tagIsInput: target?.tagName === "INPUT" || target?.tagName === "TEXTAREA",
        },
        api,
      );
      if (verdict.handled) event.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Window resize also rides the belt-and-braces command (the native
  // Resized event is the primary lane; this covers live drags).
  useEffect(() => {
    const onResize = () => {
      if (stripRef.current) {
        const rect = stripRef.current.getBoundingClientRect();
        void reportChromeSize(rect.width, rect.height);
      }
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  if (!snap) {
    return <div className="chrome chrome-boot">…</div>;
  }

  const stripUsedEnd = snap.slots.length
    ? Math.max(...snap.slots.map((s) => s.x + s.width))
    : 0;

  const commitOmnibox = async () => {
    if (omniboxText == null) return;
    const outcome = await omniboxCommit(omniboxText);
    setOmniboxText(null);
    if (outcome.kind === "join") {
      setJoinNote(
        outcome.host
          ? `No local server is known at ${outcome.host}:${outcome.port} — the fleet page lists what the daemon sees.`
          : `Nothing local listens on port ${outcome.port} — the fleet page lists what the daemon sees.`,
      );
    }
  };

  const classifyNow = async () => {
    if (omniboxText == null) return;
    const request = await omniboxClassify(omniboxText);
    if (!request) return setJoinNote(null);
    if ("Internal" in request) return setJoinNote(null);
    if ("Join" in request) {
      setJoinNote(
        request.Join.host
          ? `join address — ${request.Join.host}:${request.Join.port}`
          : `port ${request.Join.port}`,
      );
      return;
    }
    setJoinNote("search");
  };

  return (
    <div className="chrome" style={{ height: snap.header_height }}>
      {/* Tab strip row — Chromium's 35+6 band; drag region on the bare
          strip, tabs above it. */}
      <div
        className="strip"
        ref={stripRef}
        data-tauri-drag-region
        onDoubleClick={(e) => {
          if ((e.target as HTMLElement).dataset.tab === undefined) {
            void shellCommand(CMD.NEW_TAB);
          }
        }}
      >
        {snap.slots.map((slot, index) => {
          const tab = snap.tabs.find((t) => t.id === slot.id);
          if (!tab) return null;
          const group = tab.group != null ? snap.groups.find((g) => g.id === tab.group) : null;
          const dragging = dragRef.current?.tab === tab.id && dragRef.current.moved;
          return (
            <div
              key={tab.id}
              data-tab
              className={[
                "tab",
                tab.active ? "tab-active" : "tab-inactive",
                slot.pinned ? "tab-pinned" : "",
                dragging ? "tab-dragging" : "",
                slot.closing ? "tab-closing" : "",
              ].join(" ")}
              style={{
                left: slot.x,
                width: slot.width,
                ...(group ? { ["--tab-group-color" as string]: GROUP_COLOR_VARS[group.color % 6] } : {}),
              }}
              title={tab.title}
              onClick={() => selectSlotIndex(index)}
              onAuxClick={(e) => {
                if (e.button === 1) {
                  e.preventDefault();
                  void shellCommand(CMD.CLOSE_TAB, { tab_id: tab.id });
                }
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ tab: tab.id, x: e.clientX, y: e.clientY });
              }}
              onPointerDown={(e) => {
                if (e.button !== 0 || slot.pinned) return;
                dragRef.current = { tab: tab.id, start_x: e.clientX, start_y: e.clientY, moved: false };
                void shellDrag("start", { tab_id: tab.id, screen_x: e.screenX, screen_y: e.screenY });
              }}
            >
              <span className="tab-title">{slot.pinned ? tab.title.slice(0, 1) : tab.title}</span>
              {tab.muted ? <span className="tab-muted" aria-label="muted" /> : null}
              {!slot.pinned ? (
                <button
                  className="tab-close"
                  aria-label={`Close ${tab.title}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    void shellCommand(CMD.CLOSE_TAB, { tab_id: tab.id });
                  }}
                >
                  ×
                </button>
              ) : null}
              {group ? <span className="tab-group-underline" /> : null}
            </div>
          );
        })}
        <button
          className="new-tab"
          style={{ left: stripUsedEnd + 4 }}
          aria-label="New tab"
          onClick={() => void shellCommand(CMD.NEW_TAB)}
        >
          +
        </button>
        <div className="window-controls">
          <button aria-label="Minimize" onClick={() => void shellCommand(CMD.WINDOW_MINIMIZE)}>—</button>
          <button aria-label="Maximize" onClick={() => void shellCommand(CMD.WINDOW_TOGGLE_MAXIMIZE)}>□</button>
          <button aria-label="Close" className="window-close" onClick={() => void shellCommand(CMD.WINDOW_CLOSE)}>×</button>
        </div>
      </div>

      {/* Toolbar row — nav arrows, the omnibox, the star. */}
      <div className="toolbar">
        <button
          className="tool"
          aria-label="Back"
          disabled={!snap.tabs.find((t) => t.id === snap.active)?.can_back}
          onClick={() => void shellCommand(CMD.NAV_BACK)}
        >
          ‹
        </button>
        <button
          className="tool"
          aria-label="Forward"
          disabled={!snap.tabs.find((t) => t.id === snap.active)?.can_forward}
          onClick={() => void shellCommand(CMD.NAV_FORWARD)}
        >
          ›
        </button>
        <button className="tool" aria-label="Reload" onClick={() => void shellCommand(CMD.RELOAD)}>
          ⟳
        </button>
        <input
          ref={omniboxRef}
          className="omnibox"
          value={omniboxText ?? snap.address}
          spellCheck={false}
          placeholder="Search servers, or type an address"
          onChange={(e) => {
            setOmniboxText(e.target.value);
            void classifyNow();
          }}
          onFocus={(e) => e.currentTarget.select()}
          onBlur={() => {
            setOmniboxText(null);
            setJoinNote(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              void commitOmnibox();
              omniboxRef.current?.blur();
            } else if (e.key === "Escape") {
              setOmniboxText(null);
              omniboxRef.current?.blur();
            }
          }}
        />
        {joinNote ? <span className="omnibox-note">{joinNote}</span> : null}
        <button
          className="tool"
          aria-label="Bookmark this tab"
          onClick={() => void shellCommand(CMD.BOOKMARK_THIS_TAB)}
        >
          ☆
        </button>
      </div>

      {/* Bookmarks bar (IDC_SHOW_BOOKMARK_BAR posture). */}
      {snap.bookmarks_bar_visible ? (
        <div className="bookmarks">
          {snap.bookmarks.length === 0 ? (
            <span className="bookmarks-empty">Ctrl+D bookmarks the tab you're on</span>
          ) : (
            snap.bookmarks.map((bookmark) => (
              <button
                key={bookmark.id}
                className="bookmark"
                title={bookmark.title}
                onClick={() =>
                  void shellCommand(CMD.NAVIGATE_ACTIVE, { destination: bookmark.destination })
                }
                onContextMenu={(e) => {
                  e.preventDefault();
                  void import("./chromeIpc").then(({ isTauri }) => {
                    if (isTauri()) {
                      void import("@tauri-apps/api/core").then(({ invoke }) =>
                        invoke("shell_bookmark_remove", { id: bookmark.id }),
                      );
                    }
                  });
                }}
              >
                {bookmark.title}
              </button>
            ))
          )}
        </div>
      ) : null}

      {/* The tab context menu — browser verbs, not dashboard verbs. */}
      {menu ? (
        <div className="context" style={{ left: menu.x, top: menu.y }} onMouseLeave={() => setMenu(null)}>
          <button onClick={() => { void shellCommand(CMD.TOGGLE_PINNED, { tab_id: menu.tab }); setMenu(null); }}>
            Pin / unpin
          </button>
          <button onClick={() => { void shellCommand(CMD.TOGGLE_MUTE, { tab_id: menu.tab }); setMenu(null); }}>
            Mute / unmute
          </button>
          <button onClick={() => { void shellCommand(CMD.DUPLICATE_TAB, { tab_id: menu.tab }); setMenu(null); }}>
            Duplicate
          </button>
          <button
            onClick={() => {
              const label = window.prompt("Group name", "group");
              if (label) void shellCommand(CMD.ADD_NEW_TAB_TO_GROUP, { tab_id: menu.tab, label });
              setMenu(null);
            }}
          >
            Add to new group
          </button>
          <button onClick={() => { void shellCommand(CMD.NEW_TAB); setMenu(null); }}>New tab</button>
        </div>
      ) : null}

      {updateAvailable ? (
        <button className="update-pill" onClick={() => void installUpdate()}>
          Update available{updateAvailable ? ` — v${updateAvailable}` : ""} — click to install
        </button>
      ) : null}
    </div>
  );
}
