// The popup overlay — the shell's application-owned menus and forms.
// Three shapes share this transparent child webview:
//
// - "tab-menu": the tab context menu (Chromium's close-context set —
//   pin, mute, duplicate, group, the scoped closes),
// - "app-menu": the three-dot browser menu (browser-level actions only:
//   zoom, bookmarks bar, developer tools, application logs, Settings —
//   server management never enters this menu),
// - "group": the naming form that replaced the OS prompt
//   (`tauri.localhost says: …` is gone; Enter confirms, Escape cancels,
//   an empty name is a validation error, never a silent group).
//
// Keyboard contract: the overlay takes focus on open; ArrowUp/Down walk
// the items, Enter activates, Escape dismisses. A click in the
// transparent gutter dismisses, as does any interaction elsewhere in
// the window (the host routes it here).

import { useCallback, useEffect, useRef, useState } from "react";
import { CMD } from "../commandIds";
import {
  IconApps,
  IconCheck,
  IconChevRight,
  IconClock,
  IconClose,
  IconDevTools,
  IconDownload,
  IconDuplicate,
  IconGear,
  IconInfo,
  IconLogs,
  IconDashboard,
  IconMuted,
  IconNewWindow,
  IconPin,
  IconPlus,
  IconZoomIn,
  IconZoomOut,
  IconZoomReset,
} from "../ui/icons";

interface PopupContext {
  kind: string;
  tab_id: number | null;
  pinned?: boolean;
  muted?: boolean;
  zoom?: number;
  bar_visible?: boolean;
  /** §54: the strip's presentation axis — the tab menu's layout verb
   *  labels itself from it. */
  vertical?: boolean;
}

const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const demoActive = (): boolean =>
  !isTauri() &&
  typeof window !== "undefined" &&
  new URLSearchParams(window.location.search).has("demo");

async function dispatch(id: number, arg?: Record<string, unknown>): Promise<void> {
  if (!isTauri()) {
    // The demo lane: verbs land in the console — the overlay's visual
    // review never mutates a host model.
    console.info(`[popup:demo] command ${id}`, arg ?? {});
    return;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("shell_command", { id, arg: arg ?? null });
}

async function closePopup(): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("shell_popup_close").catch(() => {});
}

export function PopupApp() {
  const [ctx, setCtx] = useState<PopupContext | null>(null);
  const [groupKind, setGroupKind] = useState(false); // tab-menu → group form
  const [groupName, setGroupName] = useState("");
  const [groupError, setGroupError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  // Boot: the host pushes the context (kind + the addressed tab's
  // posture); the demo lane reads it from the query for review.
  useEffect(() => {
    if (demoActive()) {
      const kind = new URLSearchParams(window.location.search).get("kind") ?? "tab-menu";
      setCtx({
        kind,
        tab_id: 3,
        pinned: false,
        muted: false,
        zoom: 1,
        bar_visible: false,
      });
      return;
    }
    let disposed = false;
    void (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      try {
        const context = await invoke<PopupContext>("shell_popup_boot");
        // The closure-set flag reads through a box: the lint's flow
        // analysis can't see the cleanup's write, and the overlay's
        // unmount must not win a race with the boot's reply.
        if (!(disposed as boolean)) setCtx(context);
      } catch {
        // The popup died before its boot answered; the host tears it
        // down, nothing to render.
      }
    })();
    return () => {
      disposed = true;
    };
  }, []);

  const kind = groupKind ? "group" : (ctx?.kind ?? "tab-menu");

  // The group form autofocuses its input (and re-focuses it when the
  // menu turns into the form).
  useEffect(() => {
    if (kind === "group") inputRef.current?.focus();
  }, [kind]);

  // The keyboard contract: Escape dismisses; the menu items rove with
  // the arrow keys (the browser's own tab order covers the form).
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        // The form's Escape cancels the form first, then the popup —
        // "Add to new group…" returns to the menu it came from.
        if (groupKind) {
          setGroupKind(false);
          setGroupError(null);
          setGroupName("");
          return;
        }
        void closePopup();
        return;
      }
      if (kind !== "group" && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
        event.preventDefault();
        const items = [
          ...(menuRef.current?.querySelectorAll<HTMLButtonElement>(".menu-item:not([aria-disabled='true'])") ?? []),
        ];
        if (items.length === 0) return;
        const index = items.findIndex((item) => item === document.activeElement);
        const next =
          event.key === "ArrowDown"
            ? items[(index + 1) % items.length]
            : items[(index - 1 + items.length) % items.length];
        next?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [groupKind, kind]);

  const run = useCallback(async (id: number, arg?: Record<string, unknown>) => {
    await dispatch(id, arg);
    await closePopup();
  }, []);

  // A click in the gutter (the transparent area around the menu) is an
  // outside click: dismiss. The menu itself stops the propagation.
  const onGutterDown = useCallback(() => {
    void closePopup();
  }, []);

  if (!ctx) return <div className="gutter" onPointerDown={onGutterDown} />;

  const zoomPct = Math.round((ctx.zoom ?? 1) * 100);

  return (
    <div className="gutter" onPointerDown={onGutterDown}>
      {kind === "group" ? (
        <div
          className="menu group-form"
          role="dialog"
          aria-label="Name the new group"
          onPointerDown={(e) => e.stopPropagation()}
        >
          <h2>Name the group</h2>
          <input
            ref={inputRef}
            value={groupName}
            placeholder="Group name"
            maxLength={40}
            aria-invalid={groupError ? true : undefined}
            aria-describedby={groupError ? "group-error" : undefined}
            onChange={(e) => {
              setGroupName(e.target.value);
              setGroupError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                const label = groupName.trim();
                if (label === "") {
                  setGroupError("Give the group a name.");
                  return;
                }
                void dispatch(CMD.ADD_NEW_TAB_TO_GROUP, {
                  tab_id: ctx.tab_id,
                  label,
                }).then(closePopup);
              }
            }}
          />
          <p className="group-error" id="group-error" role={groupError ? "alert" : undefined}>
            {groupError ?? ""}
          </p>
          <div className="group-actions">
            <button
              className="ghost"
              onClick={() => {
                if (ctx.kind === "tab-menu") {
                  setGroupKind(false);
                  setGroupError(null);
                  setGroupName("");
                } else {
                  void closePopup();
                }
              }}
            >
              Cancel
            </button>
            <button
              className="primary"
              disabled={groupName.trim() === ""}
              onClick={() => {
                const label = groupName.trim();
                if (label === "") {
                  setGroupError("Give the group a name.");
                  return;
                }
                void dispatch(CMD.ADD_NEW_TAB_TO_GROUP, {
                  tab_id: ctx.tab_id,
                  label,
                }).then(closePopup);
              }}
            >
              Create group
            </button>
          </div>
        </div>
      ) : kind === "tab-menu" ? (
        <div
          className="menu"
          ref={menuRef}
          role="menu"
          onPointerDown={(e) => e.stopPropagation()}
        >
          <MenuItem
            glyph={<IconPlus />}
            label="New tab to the right"
            onClick={() => void run(CMD.NEW_TAB)}
          />
          <MenuItem
            glyph={<IconPin />}
            label={ctx.pinned ? "Unpin tab" : "Pin tab"}
            onClick={() => void run(CMD.TOGGLE_PINNED, { tab_id: ctx.tab_id })}
          />
          <MenuItem
            glyph={<IconMuted />}
            label={ctx.muted ? "Unmute tab" : "Mute tab"}
            onClick={() => void run(CMD.TOGGLE_MUTE, { tab_id: ctx.tab_id })}
          />
          <MenuItem
            glyph={<IconDuplicate />}
            label="Duplicate"
            onClick={() => void run(CMD.DUPLICATE_TAB, { tab_id: ctx.tab_id })}
          />
          <MenuItem
            glyph={<IconDashboard />}
            label={ctx.vertical ? "Use horizontal strip" : "Show tabs vertically"}
            onClick={() => void run(CMD.TOGGLE_VERTICAL_STRIP)}
            tick={ctx.vertical}
          />
          <MenuItem
            glyph={<IconChevRight />}
            label="Add to new group…"
            hint="›"
            onClick={() => {
              // The menu turns into the naming form in place — the same
              // overlay, one popup, no OS prompt anywhere.
              setGroupKind(true);
            }}
          />
          <div className="menu-sep" />
          <MenuItem
            glyph={<IconClose />}
            label="Close tab"
            onClick={() => void run(CMD.CLOSE_TAB, { tab_id: ctx.tab_id })}
          />
          <MenuItem
            glyph={null}
            label="Close other tabs"
            onClick={() => void run(CMD.CLOSE_OTHER_TABS, { tab_id: ctx.tab_id })}
          />
          <MenuItem
            glyph={null}
            label="Close tabs to the right"
            onClick={() => void run(CMD.CLOSE_TABS_TO_THE_RIGHT, { tab_id: ctx.tab_id })}
          />
        </div>
      ) : (
        <div
          className="menu"
          ref={menuRef}
          role="menu"
          onPointerDown={(e) => e.stopPropagation()}
        >
          <MenuItem
            glyph={<IconPlus />}
            label="New tab"
            hint="Ctrl+T"
            onClick={() => void run(CMD.NEW_TAB)}
          />
          <MenuItem
            glyph={<IconNewWindow />}
            label="New window"
            onClick={() => void run(CMD.NEW_WINDOW)}
          />
          <div className="menu-sep" />
          <div className="menu-item zoom-row" role="group" aria-label="Zoom">
            <button
              aria-label="Zoom out"
              disabled={(ctx.zoom ?? 1) <= 0.25}
              onClick={() => void run(CMD.ZOOM_OUT)}
            >
              <IconZoomOut />
            </button>
            <span className="pct" aria-live="polite">
              {zoomPct}%
            </span>
            <button
              aria-label="Zoom in"
              disabled={(ctx.zoom ?? 1) >= 5}
              onClick={() => void run(CMD.ZOOM_IN)}
            >
              <IconZoomIn />
            </button>
            <button
              aria-label="Reset zoom"
              disabled={(ctx.zoom ?? 1) === 1}
              onClick={() => void run(CMD.ZOOM_RESET)}
            >
              <IconZoomReset />
            </button>
          </div>
          <div className="menu-sep" />
          <MenuItem
            glyph={<IconCheck />}
            label="Bookmarks bar"
            hint="Ctrl+Shift+B"
            tick={ctx.bar_visible}
            onClick={() => void run(CMD.SHOW_BOOKMARK_BAR)}
          />
          <MenuItem
            glyph={<IconDevTools />}
            label="Developer tools"
            onClick={() =>
              void run(CMD.NAVIGATE_ACTIVE, { destination: { kind: "devtools" } })
            }
          />
          <MenuItem
            glyph={<IconLogs />}
            label="Application logs"
            onClick={() => void run(CMD.OPEN_LOGS)}
          />
          <div className="menu-sep" />
          <MenuItem
            glyph={<IconDownload />}
            label="Downloads"
            onClick={() =>
              void run(CMD.NAVIGATE_ACTIVE, { destination: { kind: "downloads" } })
            }
          />
          <MenuItem
            glyph={<IconApps />}
            label="Extensions"
            onClick={() =>
              void run(CMD.NAVIGATE_ACTIVE, { destination: { kind: "extensions" } })
            }
          />
          <MenuItem
            glyph={<IconClock />}
            label="Jobs"
            onClick={() => void run(CMD.NAVIGATE_ACTIVE, { destination: { kind: "jobs" } })}
          />
          <MenuItem
            glyph={<IconGear />}
            label="Settings"
            onClick={() =>
              void run(CMD.NAVIGATE_ACTIVE, { destination: { kind: "settings" } })
            }
          />
          <div className="menu-sep" />
          <MenuItem
            glyph={<IconInfo />}
            label="About ZIM"
            onClick={() => void run(CMD.NAVIGATE_ACTIVE, { destination: { kind: "about" } })}
          />
        </div>
      )}
    </div>
  );
}

/** One menu row: an icon gutter, the label, an optional keyboard hint,
 *  and an optional checkmark slot (the toggle items' state). */
function MenuItem({
  glyph,
  label,
  hint,
  tick,
  onClick,
}: {
  glyph: React.ReactNode;
  label: string;
  hint?: string;
  tick?: boolean;
  onClick: () => void;
}) {
  return (
    <button className="menu-item" role="menuitem" onClick={onClick} type="button">
      <span className="glyph" aria-hidden>
        {glyph}
      </span>
      <span className="label">{label}</span>
      {hint ? <span className="hint">{hint}</span> : null}
      <span className="tick" aria-hidden={tick ? undefined : true}>
        {tick ? <IconCheck /> : null}
      </span>
    </button>
  );
}
