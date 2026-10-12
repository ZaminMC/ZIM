// The CONTENT layer of the browser shell (ADR-0033 Phase 1). This
// document is what a tab webview loads — one destination at a time, told
// by the Rust model, never owning the strip. The frame (tab strip,
// toolbar, bookmarks bar) lives in its own webview (frame.html) and is
// this app's VIEW of the model, not its sibling renderer.
//
// Crash isolation (§51, P0.2 criterion 7) keeps its two layers: a crashed
// VIEW is a recoverable page inside this tab (TabBoundary), and a crashed
// TAB is its own webview process — the frame and the other tabs are
// untouched by construction.

import { Suspense, lazy, useEffect, useState } from "react";
import { invokeHost, listenHost, type Unlisten } from "../integration/tauri";
import type { Destination } from "../state/destinations";
import { parseAddressInput, parseJoinTail, tabKey } from "../state/destinations";
import { navigateHost } from "../state/shellLane";
import { handleBrowserKey, type BrowserKeyApi } from "../state/browserKeys";
import { useUi } from "../state/ui";
import { useTabs } from "../state/tabs";
import { startWire } from "../state/wire";
import { useConnection } from "../state/connection";
import { openServerFile } from "../state/openFile";
import { sortedServers, useServers } from "../state/servers";
import { ServerView } from "./ServerView";
import { ConsoleView } from "./ConsoleView";
import { FleetPage } from "./FleetPage";
import { MissingPage } from "./browser/MissingPage";
import { NewTabPage } from "./browser/NewTabPage";
import { TabBoundary } from "./browser/TabCrash";
import { AppearanceStudio } from "./browser/AppearanceStudio";
import styles from "./App.module.css";

// Modals are operator-invoked overlays, not boot surfaces (lazy as
// before — the cold start ships the shell + fleet page only).
const ConnectionsModal = lazy(() =>
  import("./ConnectionsModal").then((m) => ({ default: m.ConnectionsModal })),
);
const NewServerModal = lazy(() =>
  import("./NewServerModal").then((m) => ({ default: m.NewServerModal })),
);
const Palette = lazy(() => import("./Palette").then((m) => ({ default: m.Palette })));
const JobsPage = lazy(() => import("./JobsPage").then((m) => ({ default: m.JobsPage })));
const DevToolsPage = lazy(() =>
  import("./browser/DevToolsPage").then((m) => ({ default: m.DevToolsPage })),
);
const JoinPage = lazy(() =>
  import("./browser/JoinPage").then((m) => ({ default: m.JoinPage })),
);
const AuditPage = lazy(() => import("./AuditPage").then((m) => ({ default: m.AuditPage })));
const AboutPage = lazy(() => import("./AboutPage").then((m) => ({ default: m.AboutPage })));
const FeedbackPage = lazy(() =>
  import("./FeedbackPage").then((m) => ({ default: m.FeedbackPage })),
);
const ExtensionsPage = lazy(() =>
  import("./ExtensionsPage").then((m) => ({ default: m.ExtensionsPage })),
);
const DownloadsPage = lazy(() =>
  import("./DownloadsPage").then((m) => ({ default: m.DownloadsPage })),
);
const SettingsPage = lazy(() =>
  import("./browser/SettingsPage").then((m) => ({ default: m.SettingsPage })),
);

function DestinationView({ destination }: { destination: Destination }) {
  const setNewServerOpen = useUi((s) => s.setNewServerOpen);
  const serverMap = useServers((s) => s.servers);
  switch (destination.kind) {
    case "devtools":
      return <DevToolsPage />;
    case "servers":
      return (
        <FleetPage
          servers={sortedServers(serverMap)}
          onOpen={(serverId) => void navigateHost({ kind: "server", serverId })}
          onNewServer={() => setNewServerOpen(true)}
        />
      );
    case "new":
      return <NewTabPage />;
    case "settings":
      return <SettingsPage />;
    case "server":
      // The URL model rides the props: the addressed page (and the files
      // path under it) is the destination's own truth — the workspace
      // follows the address, the way a browser's viewport follows a URL.
      return (
        <ServerView
          serverId={destination.serverId}
          page={destination.page}
          filePath={destination.filePath}
        />
      );
    case "console":
      return <ConsoleView serverId={destination.serverId} variant="dedicated" />;
    case "join":
      return <JoinPage host={destination.host} port={destination.port} />;
    case "jobs":
      return <JobsPage />;
    case "audit":
      return <AuditPage />;
    case "about":
      return <AboutPage />;
    case "feedback":
      return <FeedbackPage />;
    case "extensions":
      return <ExtensionsPage />;
    case "downloads":
      return <DownloadsPage />;
    case "missing":
      return <MissingPage url={destination.url} />;
  }
}

/** The dev bridge's only input: ?d=<destination url>. The desktop host
 *  speaks through shell_tab_hello / shell://tab instead. */
function destinationFromQuery(): Destination | null {
  const url = new URLSearchParams(window.location.search).get("d");
  if (!url) return null;
  const page = url.replace(/^zim:\/\//, "").split("/")[0];
  switch (page) {
    case "devtools": return { kind: "devtools" };
    case "servers": return { kind: "servers" };
    case "new": return { kind: "new" };
    case "settings": return { kind: "settings" };
    case "jobs": return { kind: "jobs" };
    case "audit": return { kind: "audit" };
    case "about": return { kind: "about" };
    case "feedback": return { kind: "feedback" };
    case "extensions": return { kind: "extensions" };
    case "downloads": return { kind: "downloads" };
    case "server": {
      // The server model's own spelling: zim://server/{id}[/{page}[/{file}]]
      // — parsed by the ONE address law (parseAddressInput), never a
      // second slice: the page dialect must stay identical on both lanes.
      const request = parseAddressInput(url);
      if (request.kind === "internal" && request.destination.kind === "server") {
        return request.destination;
      }
      return { kind: "missing", url };
    }
    case "console": {
      const serverId = url.split("/")[3] ?? url.split("/")[2];
      return serverId ? { kind: "console", serverId } : null;
    }
    case "join": {
      // "host:port" — the host side may be empty (the port-only dialect).
      // parseJoinTail owns the spelling: the last colon is the port, a
      // colon-less tail is not a produced URL (the old slice would have
      // invented a host from the port's own digits).
      const tail = url.split("/")[3] ?? url.split("/")[2] ?? "";
      return parseJoinTail(tail) ?? { kind: "missing", url };
    }
    default: return { kind: "missing", url };
  }
}

const onHost = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function App() {
  // One tab's destination — the model says, this view renders.
  const [destination, setDestination] = useState<Destination>(
    () => destinationFromQuery() ?? { kind: "new" },
  );
  const [reloadToken, setReloadToken] = useState(0);
  const newServerOpen = useUi((s) => s.newServerOpen);
  const paletteOpen = useUi((s) => s.paletteOpen);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  // The wire's posture, mirrored for the banner: the panel must never
  // look alive while its daemon is unreachable, and the operator must
  // never stare at a broken view without the words for it.
  const wireStatus = useConnection((s) => s.status);
  // THE DWELL LAW (the founder's "the user should never know there is
  // even a connection"): a sub-second wire blip — a heartbeat edge, one
  // retry spin — is the wire's own business, never the operator's
  // banner. The words appear only after the wire has been continuously
  // not-ready for 1.5s; the moment it lands, they vanish (no
  // “everything is fine again” beat). A real outage still speaks; a
  // blip the client heals before the dwell lands is invisible. The
  // effect tracks the READY EDGE only — the retry loop's own
  // connecting↔offline flips never re-arm the dwell.
  const wireReady = wireStatus === "ready";
  const [wireDwelled, setWireDwelled] = useState(false);
  useEffect(() => {
    if (wireReady) {
      setWireDwelled(false);
      return;
    }
    const dwell = window.setTimeout(() => setWireDwelled(true), 1500);
    return () => window.clearTimeout(dwell);
  }, [wireReady, setWireDwelled]);

  useEffect(() => {
    // The palette lane attaches FIRST: the wire's absence (no daemon in
    // the test DOM) must never take the shell's lanes down with it.
    const onPalette = () => setPaletteOpen(!useUi.getState().paletteOpen);
    window.addEventListener("zamin:toggle-palette", onPalette);
    try {
      startWire();
    } catch {
      // A test DOM or an offline dev bridge: the tab still renders.
    }
    if (!onHost()) {
      return () => window.removeEventListener("zamin:toggle-palette", onPalette);
    }
    let disposed = false;
    const unlisteners: Promise<Unlisten>[] = [];

    // The §4 OS handoff, consumed exactly once per offer: the host may
    // deliver the open path through the hello reply (a webview booting
    // after the offer) or through the nudge event (a webview already
    // alive); `shell_open_claim` is the single consume point, so a
    // reload after the event cannot open the server twice.
    const takeOfferedOpen = async () => {
      try {
        const path = await invokeHost<string | null>("shell_open_claim");
        if (disposed || path === null || path.length === 0) return;
        await openServerFile(path);
      } catch {
        // The claim is best-effort: a host without the lane (older
        // build) simply answers nothing.
      }
    };

    void invokeHost<{
      fallback: boolean;
      destination?: Destination;
      reload?: number;
      query?: string | null;
      open_path?: string | null;
    }>("shell_tab_hello").then((hello) => {
      if (disposed || hello.fallback || !hello.destination) return;
      setDestination(hello.destination);
      setReloadToken(hello.reload ?? 0);
      // The omnibox's waiting search dialect (Alt-Enter's fresh tab: the
      // query landed before this webview existed, the model held it, the
      // same store the live event writes).
      if (hello.query) useTabs.getState().setDiscoveryQuery(hello.query);
      // The OS file-open dialect rides the same model-held delivery.
      if (hello.open_path) void takeOfferedOpen();
    });
    unlisteners.push(
      listenHost("shell://open-file", () => {
        void takeOfferedOpen();
      }),
    );
    unlisteners.push(
      listenHost<{
        tab_id: number;
        destination?: Destination;
        can_back: boolean;
        can_forward: boolean;
        reload?: number;
      }>("shell://tab", (tab) => {
        if (disposed) return;
        if (tab.destination) setDestination(tab.destination);
        if (tab.reload != null) setReloadToken(tab.reload);
      }),
    );
    // The omnibox's search dialect (Chromium's law: a typed query
    // navigates the CURRENT tab): the host navigated this tab to the
    // new-tab page and rings the query here — the page consumes it once
    // and runs the discovery search. The text is never dropped.
    unlisteners.push(
      listenHost<string>("shell://discover-query", (text) => {
        if (disposed) return;
        useTabs.getState().setDiscoveryQuery(text);
      }),
    );
    // The frame's Ctrl+K arrives through the host (a CustomEvent never
    // crosses webviews) — ring the palette open/closed.
    unlisteners.push(
      listenHost("shell://toggle-palette", () => {
        if (disposed) return;
        setPaletteOpen(!useUi.getState().paletteOpen);
      }),
    );
    return () => {
      disposed = true;
      for (const promise of unlisteners) void promise.then((off) => off());
      window.removeEventListener("zamin:toggle-palette", onPalette);
    };
  }, [setPaletteOpen]);

  // The content side of the keyboard contract (ADR-0032): browser
  // commands ride the host's ID space; the palette stays content-local.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as (HTMLElement & { isContentEditable: boolean }) | null;
      const api: BrowserKeyApi = {
        newTab: () => void invokeHost("shell_command", { id: 34014, arg: null }),
        reopenClosedTab: () => void invokeHost("shell_command", { id: 34028, arg: null }),
        closeActiveTab: () => void invokeHost("shell_command", { id: 34015, arg: null }),
        cycleTab: (step) =>
          void invokeHost("shell_command", { id: step >= 0 ? 34016 : 34017, arg: null }),
        selectTabIndex: (index) =>
          void invokeHost("shell_command", {
            id: index === "last" ? 50002 : 34018 + index,
            arg: null,
          }),
        focusAddressBar: () => void invokeHost("shell_command", { id: 39001, arg: null }),
        reload: () => void invokeHost("shell_command", { id: 33002, arg: null }),
        goBack: () => void invokeHost("shell_command", { id: 50006, arg: null }),
        goForward: () => void invokeHost("shell_command", { id: 50007, arg: null }),
        bookmarkActive: () => void invokeHost("shell_command", { id: 35000, arg: null }),
        toggleBookmarksBar: () => void invokeHost("shell_command", { id: 40009, arg: null }),
        togglePalette: () => setPaletteOpen(!useUi.getState().paletteOpen),
        paletteOpen: () => useUi.getState().paletteOpen,
        openDevTools: () =>
          void invokeHost("shell_command", {
            id: 50004,
            arg: { destination: { kind: "devtools" } },
          }),
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
  }, [setPaletteOpen]);

  useEffect(() => {
    // Any pointerdown here is an interaction a popup overlay must yield
    // to (cheap no-op when none is open).
    if (!onHost()) return;
    const onDown = () => void invokeHost("shell_popup_dismiss").catch(() => {});
    window.addEventListener("pointerdown", onDown, true);
    return () => window.removeEventListener("pointerdown", onDown, true);
  }, []);

  return (
    <div className={styles.content}>
      {/* The Arix stage (the theme port): the fixed background layer the
          glass surfaces blur — the ground plus the preset image, fading in
          below the frame band's matching ground so the seam is invisible.
          The class is the global glass layer's own (theme/arix.css). */}
      <div className="arix-bg" aria-hidden />
      {wireStatus !== "ready" && wireDwelled ? (
        <div className={styles.wireBanner} role="status" data-state={wireStatus}>
          <span className={styles.wireDot} aria-hidden />
          <span className={styles.wireText}>
            {wireStatus === "connecting"
              ? "Connecting…"
              : "Reconnecting — ZIM is not answering"}
          </span>
        </div>
      ) : null}
      <TabBoundary>
        <Suspense fallback={<div className={styles.lazyFallback} />}>
          {/* The page band: one destination at a time, keyed by the
              destination's TAB IDENTITY and the reload token. The key is
              §60's law — the reload verb rebuilds the view on EVERY
              page (the evidence and settings pages used to ignore the
              token, so ⟳ did nothing there). The identity — not the
              full destination — keys the band because the URL model
              (§8–§12) walks a server's PAGES through history: the
              tab is the same document family, so the walk must UPDATE
              in place (state, scroll, subscriptions preserved), never
              remount per page; a different destination (another server,
              an evidence page) is a different identity and remounts.
              The swap is Chromium's paint holding (App.module.css): one
              atomic commit, no blank beat — the old page's paint stands
              until the new page's lands. */}
          <div
            className={`${styles.page} page-enter`}
            key={`${tabKey(destination)}:${reloadToken}`}
          >
            <DestinationView destination={destination} />
          </div>
        </Suspense>
      </TabBoundary>
      <Suspense fallback={null}>
        {newServerOpen ? <NewServerModal /> : null}
        {paletteOpen ? <Palette /> : null}
        <ConnectionsModal />
      </Suspense>
      {/* The customization studio (the Arix theme port): the new tab's
          bottom-right icon opens it; it renders on every destination so a
          theme change never depends on which page is showing. */}
      <AppearanceStudio />
    </div>
  );
}
