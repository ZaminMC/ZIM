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
import { navigateHost } from "../state/shellLane";
import { handleBrowserKey, type BrowserKeyApi } from "../state/browserKeys";
import { useUi } from "../state/ui";
import { startWire } from "../state/wire";
import { useConnection } from "../state/connection";
import { sortedServers, useServers } from "../state/servers";
import { ServerView } from "./ServerView";
import { ConsoleView } from "./ConsoleView";
import { FleetPage } from "./FleetPage";
import { MissingPage } from "./browser/MissingPage";
import { NewTabPage } from "./browser/NewTabPage";
import { TabBoundary } from "./browser/TabCrash";
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

function DestinationView({ destination, reloadToken }: { destination: Destination; reloadToken: number }) {
  const setNewServerOpen = useUi((s) => s.setNewServerOpen);
  const serverMap = useServers((s) => s.servers);
  const key = `${JSON.stringify(destination)}:${reloadToken}`;
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
      return <NewTabPage key={key} />;
    case "settings":
      return <SettingsPage />;
    case "server":
      return <ServerView key={key} serverId={destination.serverId} />;
    case "console":
      return <ConsoleView key={key} serverId={destination.serverId} variant="dedicated" />;
    case "join":
      return <JoinPage key={key} host={destination.host} port={destination.port} />;
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
      const serverId = url.split("/")[3] ?? url.split("/")[2];
      return serverId ? { kind: "server", serverId } : null;
    }
    case "console": {
      const serverId = url.split("/")[3] ?? url.split("/")[2];
      return serverId ? { kind: "console", serverId } : null;
    }
    case "join": {
      // "host:port" — the host side may be empty (the port-only dialect).
      const tail = url.split("/")[3] ?? url.split("/")[2] ?? "";
      const sep = tail.lastIndexOf(":");
      const port = Number(tail.slice(sep + 1));
      if (!Number.isInteger(port) || port <= 0 || port > 65535) return { kind: "missing", url };
      const host = tail.slice(0, sep);
      return { kind: "join", ...(host === "" ? {} : { host }), port };
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
    void invokeHost<{ fallback: boolean; destination?: Destination; reload?: number }>(
      "shell_tab_hello",
    ).then((hello) => {
      if (disposed || hello.fallback || !hello.destination) return;
      setDestination(hello.destination);
      setReloadToken(hello.reload ?? 0);
    });
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
      {wireStatus !== "ready" ? (
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
          <DestinationView destination={destination} reloadToken={reloadToken} />
        </Suspense>
      </TabBoundary>
      <Suspense fallback={null}>
        {newServerOpen ? <NewServerModal /> : null}
        {paletteOpen ? <Palette /> : null}
        <ConnectionsModal />
      </Suspense>
    </div>
  );
}
