// The frame (ADR-0033 Phase 1): the VIEW of the Rust shell model. This
// document is the window's frame band — tab strip, toolbar, omnibox,
// bookmarks bar, window controls — and nothing else. Content lives in
// its own webview per tab. Every metric and interaction here renders
// from the host's snapshot: the layout law runs in the model
// (shell/layout.rs); this layer is deliberately dumb geometry and
// pointer reporting.
//
// Ported behaviors (upstream citations live in shell/ and the porting
// ledger):
// - tab slots from the width law (tab_width_constraints.cc),
// - drag: start→move→drop into the ported TabDragController (15 DIP
//   detach magnetism; reorder by slot centers),
// - commands: the ported command ID space (chrome_command_ids.h) via
//   one dispatch,
// - omnibox: the model classifies on every keystroke (autocomplete_input).

import { useCallback, useEffect, useRef, useState } from "react";
import {
  bootFrame,
  dismissPopup,
  dropIndexFromSlots,
  isTauri,
  omniboxClassify,
  omniboxCommit,
  onSnapshot,
  onFocusAddress,
  reportFrameSize,
  revealSlot,
  shellCommand,
  shellDrag,
  shellPopup,
  stripScroll,
  type Snapshot,
} from "./frameIpc";
import { handleBrowserKey, type BrowserKeyApi } from "../state/browserKeys";
import { CMD } from "../commandIds";
import {
  IconBack,
  IconForward,
  IconReload,
  IconStar,
  IconPlus,
  IconClose,
  IconMuted,
  IconMinimize,
  IconMaximize,
  IconWindowClose,
  IconZim,
  IconServer,
  IconTerminal,
  IconGear,
  IconClock,
  IconShield,
  IconInfo,
  IconChat,
  IconApps,
  IconDownload,
  IconGlobe,
  IconChevLeft,
  IconChevRight,
  IconDots,
} from "./icons";
import "./frame.css";

const GROUP_COLOR_VARS = [
  "var(--group-sky)",
  "var(--group-grass)",
  "var(--group-amber)",
  "var(--group-rose)",
  "var(--group-violet)",
  "var(--group-slate)",
];

// The favicon lane: the frame knows each destination's kind from its
// zim:// URL, so a glyph stands where the site's icon will ride later.
// Pure view mapping — the model stays unaware of pictures.
type IconComponent = (props: React.SVGProps<SVGSVGElement>) => React.JSX.Element;

const DEST_GLYPHS: Record<string, IconComponent> = {
  servers: IconServer,
  server: IconServer,
  console: IconTerminal,
  settings: IconGear,
  jobs: IconClock,
  audit: IconShield,
  about: IconInfo,
  feedback: IconChat,
  extensions: IconApps,
  downloads: IconDownload,
};

function destinationGlyph(url: string): IconComponent {
  if (url === "" || url === "zim://new") return IconZim;
  const kind = /^zim:\/\/([^/?#]+)/.exec(url)?.[1];
  return (kind && DEST_GLYPHS[kind]) || IconGlobe;
}

function Glyph({ url }: { url: string }) {
  const Icon = destinationGlyph(url);
  return <Icon />;
}

export function FrameApp() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const stripRef = useRef<HTMLDivElement | null>(null);
  const omniboxRef = useRef<HTMLInputElement | null>(null);
  const dragRef = useRef<{ tab: number; start_x: number; start_y: number; moved: boolean } | null>(null);
  const [menu, setMenu] = useState<{ tab: number; x: number; y: number } | null>(null);
  const [omniboxText, setOmniboxText] = useState<string | null>(null);
  const [joinNote, setJoinNote] = useState<string | null>(null);
  const [updateAvailable, setUpdateAvailable] = useState<string | null>(null);
  // The boot must never fail silently: a white window teaches nothing.
  // Three spaced retries, then an honest error panel with a manual retry.
  const [bootError, setBootError] = useState<string | null>(null);
  // The strip's scroll posture (tab_strip scrolling): the model's layout
  // law shrinks the tabs first; when even the minimum run overflows, the
  // lane translates. A ref mirror rides along so the drag session's drop
  // can convert view coordinates to model coordinates without a render.
  const [scroll, setScroll] = useState(0);
  const scrollRef = useRef(0);
  scrollRef.current = scroll;
  // The live drag session's pointer (frame coordinates), non-null only
  // once the drag crosses its threshold — the render reads it to lift
  // the dragged tab and place the insertion indicator.
  const [dragPointer, setDragPointer] = useState<{ x: number; y: number } | null>(null);
  // When a drag session released: its pointerup also dispatches a click,
  // and the release must never select the tab it just dragged.
  const dragJustEnded = useRef(0);
  // The demo menu's inline group naming (the popup overlay's form in the
  // real shell; the demo has no popup host).
  const [demoGroupFor, setDemoGroupFor] = useState<number | null>(null);

  // The model's tab order is snapshot.tabs' order (the host maps it
  // straight from strip.tabs) — the slot view interleaves group chips
  // and hides collapsed tabs, so positions must come from the model's
  // array, never from the rendered slot sequence.
  const selectTab = useCallback(
    (tabId: number) => {
      if (snap) {
        const index = snap.tabs.findIndex((t) => t.id === tabId);
        if (index >= 0) void shellCommand(CMD.SELECT_TAB_0 + index);
      }
    },
    [snap],
  );

  // The drag session — Chromium's TabDragController, frame side. A
  // pointerdown captures the pointer, a 10 DIP threshold arms the drag
  // (the porting spec's threshold), and from there the session reports
  // rAF-coalesced moves (the host's detach magnetism watches y), lifts
  // the tab, and places the insertion indicator over a drop-index mirror.
  // Drop: view x converts to model x (the lane's scroll), then the host
  // reorders — or tears off when the pointer left the strip.
  const startDragSession = useCallback((event: React.PointerEvent, tabId: number) => {
    if (event.button !== 0) return;
    const startX = event.clientX;
    const startY = event.clientY;
    let moved = false;
    let raf = 0;
    let last = { x: event.clientX, y: event.clientY, sx: event.screenX, sy: event.screenY };
    dragRef.current = { tab: tabId, start_x: startX, start_y: startY, moved: false };
    // Capture: the session must survive the pointer leaving the webview —
    // a tear-off drop lands past the window's edge.
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      // A failed capture still drags inside the window; the drop just
      // loses its beyond-the-edge reach.
    }
    void shellDrag("start", { tab_id: tabId, screen_x: event.screenX, screen_y: event.screenY });

    const onMove = (ev: PointerEvent) => {
      last = { x: ev.clientX, y: ev.clientY, sx: ev.screenX, sy: ev.screenY };
      if (!moved && Math.hypot(ev.clientX - startX, ev.clientY - startY) > 10) {
        moved = true;
        if (dragRef.current) dragRef.current.moved = true;
      }
      if (!moved) return;
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        setDragPointer({ x: last.x, y: last.y });
        void shellDrag("move", {
          tab_id: tabId,
          x: last.x,
          y: last.y,
          screen_x: last.sx,
          screen_y: last.sy,
        });
      });
    };
    const finish = (commit: boolean) => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("keydown", onKey);
      cancelAnimationFrame(raf);
      dragRef.current = null;
      setDragPointer(null);
      if (commit && moved) dragJustEnded.current = performance.now();
      if (commit) {
        void shellDrag("drop", {
          tab_id: tabId,
          x: last.x + scrollRef.current, // view → model: the lane's shift
          y: last.y,
          screen_x: last.sx,
          screen_y: last.sy,
        });
      } else {
        void shellDrag("cancel", { tab_id: tabId });
      }
    };
    const onUp = () => finish(true);
    const onCancel = () => finish(false);
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") finish(false);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    window.addEventListener("keydown", onKey);
  }, []);

  const boot = useCallback((attempt = 0) => {
    bootFrame()
      .then((first) => {
        if (first) {
          setSnap(first);
          setBootError(null);
        } else if (!isTauri()) {
          // No host answered and no demo fixture asked for the stage:
          // say so — the frame must never sit as a silent ellipsis.
          setBootError(
            "no shell answered. Run the desktop app (the frame is its guest), or open this page with ?demo for the browser fixture.",
          );
        }
      })
      .catch((error: unknown) => {
        if (attempt < 2) {
          window.setTimeout(() => boot(attempt + 1), 250 * (attempt + 1));
        } else {
          setBootError(
            error instanceof Error
              ? error.message
              : typeof error === "string" && error !== ""
                ? error
                : "the shell did not answer",
          );
        }
      });
  }, []);

  // Boot + the snapshot lane (the model pushes every change; a pushed
  // snapshot also rescues a frame whose boot answer was lost).
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    boot();
    void onSnapshot((next) => {
      if (disposed) return;
      setSnap(next);
      setBootError(null);
    }).then((off) => {
      if (disposed) off();
      else unlisten = off;
    });
    // The model needs the frame's true width for the layout law. The
    // first report must ride the snapshot's arrival: at mount the strip
    // does not exist yet (snap is null), so a mount-only report raced
    // its own element and the model kept a stale width until some
    // unrelated resize.
    const report = () => {
      if (stripRef.current) {
        const rect = stripRef.current.getBoundingClientRect();
        void reportFrameSize(rect.width, rect.height);
      }
    };
    report();
    window.addEventListener("resize", report);
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("resize", report);
    };
  }, [boot]);

  // The strip's re-measure whenever it (re)appears: boot, bookmarks-bar
  // posture flips, any snapshot that changes the band's shape.
  useEffect(() => {
    if (!snap) return;
    const frame = window.requestAnimationFrame(() => {
      if (stripRef.current) {
        const rect = stripRef.current.getBoundingClientRect();
        void reportFrameSize(rect.width, rect.height);
      }
    });
    return () => window.cancelAnimationFrame(frame);
  }, [snap?.header_height, snap?.bookmarks_bar_visible]);

  // The reveal law + the scroll clamp, applied on every snapshot: the
  // active tab must be visible (Chrome scrolls just enough), and the
  // lane's shift can never outrun the layout's own maximum. The setter
  // bails out when the value is unchanged, so this is safe on every
  // snapshot, not only on selection changes.
  useEffect(() => {
    if (!snap) return;
    const { max } = stripScroll(snap.slots, snap.strip_width, 0);
    const activeSlot = snap.slots.find((s) => !s.header && s.id === snap.active);
    setScroll((cur) => {
      const clamped = Math.min(cur, max);
      return revealSlot(activeSlot, snap.strip_width, clamped, max) === clamped
        ? clamped
        : revealSlot(activeSlot, snap.strip_width, clamped, max);
    });
  }, [snap]);

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

  // Any pointerdown in the frame is an interaction a popup must yield
  // to (the anchor button re-opens on the click that follows — the
  // upstream toggle rhythm). A cheap no-op when nothing is open.
  useEffect(() => {
    if (!isTauri()) return;
    const onDown = () => void dismissPopup();
    window.addEventListener("pointerdown", onDown, true);
    return () => window.removeEventListener("pointerdown", onDown, true);
  }, []);

  // The omnibox rests on the model's address until the user edits it.
  useEffect(() => {
    setOmniboxText(null);
    setJoinNote(null);
  }, [snap?.address, snap?.active]);

  // The DOM menu (demo stand-in) closes on Escape or any click outside
  // itself — the native menu gets this from the OS for free.
  useEffect(() => {
    if (!menu) return;
    const onDown = (e: PointerEvent) => {
      if (!(e.target as HTMLElement | null)?.closest(".context")) setMenu(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenu(null);
    };
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  // The update lane (ADR-0024): the frame owns the pill now.
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

  // The browser keyboard contract (ADR-0032), frame side.
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
          // The palette lives in the active tab's webview — a CustomEvent
          // never crosses webviews, so the frame asks the host to ring
          // the tab's bell (shell://toggle-palette).
          void shellCommand(CMD.TOGGLE_PALETTE);
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
        void reportFrameSize(rect.width, rect.height);
      }
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  if (bootError) {
    return (
      <div className="frame frame-error" role="alert">
        <div className="frame-error-box">
          <p className="frame-error-title">The shell did not boot</p>
          <p className="frame-error-detail">{bootError}</p>
          <button className="frame-error-retry" onClick={() => { setBootError(null); boot(); }}>
            Try again
          </button>
        </div>
      </div>
    );
  }

  if (!snap) {
    return <div className="frame frame-boot">…</div>;
  }

  const stripUsedEnd = snap.slots.length
    ? Math.max(...snap.slots.map((s) => s.x + s.width))
    : 0;
  // The scroll posture: the model's law shrank the tabs; whatever still
  // overflows scrolls (stripScroll's reserve mirrors the + clamp zone).
  const { max: maxScroll } = stripScroll(snap.slots, snap.strip_width, 0);
  const scrollValue = Math.min(Math.max(scroll, 0), maxScroll);
  // The + never slides under the caption area: unscrolled it follows the
  // last slot; once the strip scrolls it pins at the right reserve
  // (WINDOW_CONTROLS_W + NEW_TAB_BUTTON_W, shell/layout.rs) and the tabs
  // slide beneath it, as upstream's scrolled strip does.
  const newTabLeft =
    scrollValue > 0
      ? Math.max(snap.strip_width - 174, 0)
      : Math.min(stripUsedEnd + 4, Math.max(snap.strip_width - 174, 0));

  // The drag session's visuals: the lifted tab and its insertion
  // indicator. The indicator computes over the slots MINUS the dragged
  // tab (the lift-out rule the host's drop applies) and only while the
  // pointer stays in the strip band — beyond it the drop is a tear-off
  // and no in-strip insertion exists.
  const dragTabId = dragPointer != null ? (dragRef.current?.tab ?? null) : null;
  const inStripBand = dragPointer != null && dragPointer.y <= 41 + 15;
  let insertX: number | null = null;
  if (dragTabId != null && inStripBand) {
    const others = snap.slots.filter((s) => s.header || s.id !== dragTabId);
    const preview = dropIndexFromSlots(others, dragPointer.x + scrollValue);
    const tabsOnly = others;
    const prev = tabsOnly[preview - 1];
    const next = tabsOnly[preview];
    if (prev && next) {
      insertX = (prev.x + prev.width + next.x) / 2;
    } else if (next) {
      insertX = Math.max(next.x - 9, 6);
    } else if (prev) {
      insertX = prev.x + prev.width - 18;
    }
  }
  // The dragged tab follows the pointer, clamped to the window's span
  // (translate is rigid, so local and visual deltas agree).
  const dragDx = (slotX: number, width: number): number | null => {
    if (dragTabId == null || dragPointer == null) return null;
    const visualX = slotX - scrollValue;
    const raw = dragPointer.x - (dragRef.current?.start_x ?? dragPointer.x);
    const min = -(visualX - 6);
    const maxDx = snap.strip_width - visualX - width + 12;
    return Math.min(Math.max(raw, min), Math.max(min, maxDx));
  };

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
    <div className="frame" style={{ height: snap.header_height }}>
      {/* Tab strip row — Chromium's 35+6 band; drag region on the bare
          strip, tabs above it. */}
      <div
        className="strip"
        ref={stripRef}
        data-tauri-drag-region
        onDoubleClick={(e) => {
          if ((e.target as HTMLElement).dataset.tab === undefined) {
            // Windows titlebar law: a bare-strip double click asks about
            // the window, never about tabs (the + button and Ctrl+T
            // make tabs).
            void shellCommand(CMD.WINDOW_TOGGLE_MAXIMIZE);
          }
        }}
      >
        {/* The overflow lane — the model's slots translate inside it
            while the strip's own chrome stays put. The lane is
            pointer-transparent: bare areas remain the window's drag
            region and the double-click maximize law keeps working. */}
        <div
          className="strip-lane"
          style={{ transform: `translateX(${-scrollValue}px)` }}
          onWheel={(e) => {
            if (maxScroll === 0) return;
            const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
            setScroll(stripScroll(snap.slots, snap.strip_width, scrollRef.current + delta).value);
          }}
        >
          {snap.slots.flatMap((slot, idx) => {
            const parts: React.JSX.Element[] = [];
            // Separators: the VIEW owns the adjacency truth — an
            // explicit 2×20 mark between two adjacent inactive tab
            // slots (the old CSS sibling selector could not see across
            // a chip or an absolutely-positioned run). The span sits
            // BETWEEN the tab divs in DOM order so hover of either
            // neighbor hides it through the sibling/:has() rules.
            const prevSlot = idx > 0 ? snap.slots[idx - 1] : undefined;
            if (prevSlot && !slot.header && !prevSlot.header && !slot.closing && !prevSlot.closing) {
              const prevTab = snap.tabs.find((t) => t.id === prevSlot.id);
              const curTab = snap.tabs.find((t) => t.id === slot.id);
              if (prevTab && curTab && !prevTab.active && !curTab.active) {
                parts.push(
                  <span
                    key={`sep-${slot.id}`}
                    className="tab-separator"
                    style={{ left: slot.x - 1 }}
                  />,
                );
              }
            }
            // A header slot is the group's chip — not a tab. Clicking it
            // toggles the group's collapse (the chip IS the collapsed
            // group, tab_group_views.cc).
            if (slot.header) {
              const group = snap.groups.find((g) => g.id === slot.id);
              if (!group) return parts;
              parts.push(
                <button
                  key={`group-${group.id}`}
                  data-group-chip
                  className="group-chip"
                  style={{
                    left: slot.x,
                    width: slot.width,
                    ["--tab-group-color" as string]: GROUP_COLOR_VARS[group.color % 6],
                  }}
                  title={`Group ${group.label}${group.collapsed ? " — collapsed" : ""}`}
                  aria-label={`Toggle group ${group.label}`}
                  onClick={() =>
                    void shellCommand(CMD.TOGGLE_GROUP_COLLAPSE, { group_id: group.id })
                  }
                >
                  <span className="group-chip-label">{group.label}</span>
                </button>,
              );
              return parts;
            }
            const tab = snap.tabs.find((t) => t.id === slot.id);
            if (!tab) return parts;
            const group = tab.group != null ? snap.groups.find((g) => g.id === tab.group) : null;
            // The drag session's lift: the moved tab follows the pointer
            // (clamped to the window) with the settle transitions off.
            const dragging = dragTabId === tab.id;
            const dx = dragging ? dragDx(slot.x, slot.width) : null;
            // Favicon-only mode: below ~64 DIP the content insets (2 × 24)
            // cannot fit beside a glyph — the slot shows its glyph alone,
            // centered in the visible span, the way Chromium's minimum
            // tabs render (min_inactive_width, interior 16).
            const tight = slot.width < 64;
            parts.push(
              <div
                key={tab.id}
                data-tab
                className={[
                  "tab",
                  tab.active ? "tab-active" : "tab-inactive",
                  slot.pinned ? "tab-pinned" : "",
                  tight ? "tab-tight" : "",
                  dragging ? "tab-dragging tab-dragging-live" : "",
                  slot.closing ? "tab-closing" : "",
                ].join(" ")}
                style={{
                  left: slot.x,
                  width: slot.width,
                  ...(dx != null
                    ? { transform: `translateX(${dx}px)`, transition: "none", willChange: "transform" }
                    : {}),
                  ...(group ? { ["--tab-group-color" as string]: GROUP_COLOR_VARS[group.color % 6] } : {}),
                }}
                title={tab.title}
                onClick={() => {
                  // A drag session's release also dispatches a click —
                  // the just-ended drag never selects.
                  if (performance.now() - dragJustEnded.current < 200) return;
                  selectTab(tab.id);
                }}
                onAuxClick={(e) => {
                  if (e.button === 1) {
                    e.preventDefault();
                    void shellCommand(CMD.CLOSE_TAB, { tab_id: tab.id });
                  }
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  // The application-owned popup overlay (a transparent
                  // child webview) — the native gray menu is gone. In
                  // the browser demo the DOM stand-in still serves.
                  if (isTauri()) {
                    void shellPopup("tab-menu", tab.id, e.clientX, e.clientY);
                  } else {
                    setMenu({ tab: tab.id, x: e.clientX, y: e.clientY });
                  }
                }}
                onPointerDown={(e) => startDragSession(e, tab.id)}
              >
                <span className="tab-glyph" aria-hidden>
                  <Glyph url={tab.url} />
                </span>
                {!slot.pinned ? <span className="tab-title">{tab.title}</span> : null}
                {tab.muted ? (
                  <span className="tab-muted" aria-label="muted">
                    <IconMuted />
                  </span>
                ) : null}
                {!slot.pinned ? (
                  <button
                    className="tab-close"
                    aria-label={`Close ${tab.title}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      void shellCommand(CMD.CLOSE_TAB, { tab_id: tab.id });
                    }}
                  >
                    <IconClose />
                  </button>
                ) : null}
                {group ? <span className="tab-group-underline" /> : null}
              </div>,
            );
            return parts;
          })}
          {/* The insertion indicator — the drag session's drop preview
              (DragInsertionIndicator): a 2px accent bar at the boundary
              the drop would land on. It only exists while the pointer
              stays in the strip band; beyond it the drop tears off. */}
          {insertX != null ? <div className="drag-insert" style={{ left: insertX }} /> : null}
        </div>
        {/* Scroll chevrons (tab_strip scrolling): only while the layout
            overflows its minimum run, riding the strip's right reserve. */}
        {maxScroll > 0 ? (
          <>
            <button
              className="strip-chev"
              style={{ left: Math.max(snap.strip_width - 174 - 58, 0) }}
              aria-label="Scroll tabs left"
              disabled={scrollValue <= 0}
              onClick={() =>
                setScroll(stripScroll(snap.slots, snap.strip_width, scrollValue - 240).value)
              }
            >
              <IconChevLeft />
            </button>
            <button
              className="strip-chev"
              style={{ left: Math.max(snap.strip_width - 174 - 30, 0) }}
              aria-label="Scroll tabs right"
              disabled={scrollValue >= maxScroll}
              onClick={() =>
                setScroll(stripScroll(snap.slots, snap.strip_width, scrollValue + 240).value)
              }
            >
              <IconChevRight />
            </button>
          </>
        ) : null}
        <button
          className="new-tab"
          style={{ left: newTabLeft }}
          aria-label="New tab"
          onClick={() => void shellCommand(CMD.NEW_TAB)}
        >
          <IconPlus />
        </button>
        <div className="window-controls">
          <button aria-label="Minimize" onClick={() => void shellCommand(CMD.WINDOW_MINIMIZE)}>
            <IconMinimize />
          </button>
          <button aria-label="Maximize" onClick={() => void shellCommand(CMD.WINDOW_TOGGLE_MAXIMIZE)}>
            <IconMaximize />
          </button>
          <button aria-label="Close" className="window-close" onClick={() => void shellCommand(CMD.WINDOW_CLOSE)}>
            <IconWindowClose />
          </button>
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
          <IconBack />
        </button>
        <button
          className="tool"
          aria-label="Forward"
          disabled={!snap.tabs.find((t) => t.id === snap.active)?.can_forward}
          onClick={() => void shellCommand(CMD.NAV_FORWARD)}
        >
          <IconForward />
        </button>
        <button className="tool" aria-label="Reload" onClick={() => void shellCommand(CMD.RELOAD)}>
          <IconReload />
        </button>
        <div className="omnibox-wrap">
          <span className="omnibox-icon">
            <Glyph url={snap.address} />
          </span>
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
          {/* The bookmark star lives INSIDE the field's right end — the
              placement the omnibox owns upstream; it never sits as a
              stray button past the field. */}
          <button
            className="omnibox-star"
            aria-label="Bookmark this tab"
            title="Bookmark this tab (Ctrl+D)"
            onClick={() => void shellCommand(CMD.BOOKMARK_THIS_TAB)}
          >
            <IconStar />
          </button>
        </div>
        {/* The three-dot menu — the browser-level actions live here and
            nowhere else; server management stays in the server's own
            views. The popup overlay anchors under the button. */}
        <button
          className="tool"
          aria-label="Customize and control ZIM"
          title="Customize and control ZIM"
          onClick={(e) => {
            const rect = e.currentTarget.getBoundingClientRect();
            void shellPopup("app-menu", null, rect.left, rect.bottom + 4);
          }}
        >
          <IconDots />
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
                  void import("./frameIpc").then(({ isTauri }) => {
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

      {/* The tab context menu — the DEMO stand-in only (in the demo the
          group item grows the inline naming input; under the host this
          whole surface is the popup overlay). */}
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
          {demoGroupFor === menu.tab ? (
            <input
              className="context-group-input"
              autoFocus
              placeholder="Group name"
              maxLength={40}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  const label = e.currentTarget.value.trim();
                  if (label === "") return;
                  void shellCommand(CMD.ADD_NEW_TAB_TO_GROUP, { tab_id: menu.tab, label });
                  setDemoGroupFor(null);
                  setMenu(null);
                } else if (e.key === "Escape") {
                  setDemoGroupFor(null);
                  setMenu(null);
                }
              }}
            />
          ) : (
            <button onClick={() => setDemoGroupFor(menu.tab)}>
              Add to new group
            </button>
          )}
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
