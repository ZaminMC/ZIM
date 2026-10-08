// The browser shell (ADR-0015, ADR-0016): a tab strip, a tool bar whose
// address bar speaks the product's dialects, and one destination at a
// time. The sidebar is gone — ZaminPanel is a browser for Minecraft
// servers. Every view below the chrome is the workspace that already
// existed; the frame is what changed. §51: the mounted tab content sits
// inside an isolation boundary — a crashed view is a recoverable page,
// never a dead shell.

import { Suspense, lazy, useEffect, useMemo } from "react";
import type { Destination } from "../state/destinations";
import { destinationLabel } from "../state/destinations";
import { handleBrowserKey, type BrowserKeyApi } from "../state/browserKeys";
import { isBookmarked, useBookmarks } from "../state/bookmarks";
import { sortedServers, useServers } from "../state/servers";
import { bootWindow, tabDestination, tabKeyOf, useTabs } from "../state/tabs";
import { startUpdates } from "../state/updates";
import { useUi } from "../state/ui";
import { startWire } from "../state/wire";
import { ServerView } from "./ServerView";
import { ConsoleView } from "./ConsoleView";
import { FleetPage } from "./FleetPage";
import { MissingPage } from "./browser/MissingPage";
import { BookmarksBar } from "./browser/BookmarksBar";
import { NewTabPage } from "./browser/NewTabPage";
import { TabBoundary } from "./browser/TabCrash";
import { TabStrip } from "./browser/TabStrip";
import { ToolBar } from "./browser/ToolBar";
import { UpdateNotice } from "./browser/UpdateNotice";
import styles from "./App.module.css";

// Modals are operator-invoked overlays, not boot surfaces: each loads on
// first open so the cold start ships only the shell + fleet page
// (PERFORMANCE-BUDGETS: cold start → interactive).
const ConnectionsModal = lazy(() =>
  import("./ConnectionsModal").then((m) => ({ default: m.ConnectionsModal })),
);
const NewServerModal = lazy(() =>
  import("./NewServerModal").then((m) => ({ default: m.NewServerModal })),
);
const Palette = lazy(() => import("./Palette").then((m) => ({ default: m.Palette })));
// The evidence pages (§58/§72/§73, ADR-0026) are destinations, not boot
// surfaces: each loads when its tab first opens, keeping the entry chunk
// to the shell + fleet page (PERFORMANCE-BUDGETS).
const JobsPage = lazy(() => import("./JobsPage").then((m) => ({ default: m.JobsPage })));
const AuditPage = lazy(() => import("./AuditPage").then((m) => ({ default: m.AuditPage })));
const AboutPage = lazy(() => import("./AboutPage").then((m) => ({ default: m.AboutPage })));
const FeedbackPage = lazy(() =>
  import("./FeedbackPage").then((m) => ({ default: m.FeedbackPage })),
);
// §56/§57 (ADR-0031): the extensions room — the inventory rides its own
// lazy chunk like the other internal pages.
const ExtensionsPage = lazy(() =>
  import("./ExtensionsPage").then((m) => ({ default: m.ExtensionsPage })),
);
// §58's reserved future URL, live now that a versioned channel exists
// (ADR-0029) — the downloads room rides its own lazy chunk.
const DownloadsPage = lazy(() =>
  import("./DownloadsPage").then((m) => ({ default: m.DownloadsPage })),
);
// The settings page rides the same lane: an internal page, not a boot
// surface — its tab loads it on first open (PERFORMANCE-BUDGETS).
const SettingsPage = lazy(() =>
  import("./browser/SettingsPage").then((m) => ({ default: m.SettingsPage })),
);

function DestinationView({ destination }: { destination: Destination }) {
  const navigate = useTabs((s) => s.navigate);
  const setNewServerOpen = useUi((s) => s.setNewServerOpen);
  const serverMap = useServers((s) => s.servers);
  switch (destination.kind) {
    case "servers":
      return (
        <FleetPage
          servers={sortedServers(serverMap)}
          onOpen={(serverId) => navigate({ kind: "server", serverId })}
          onNewServer={() => setNewServerOpen(true)}
        />
      );
    case "new":
      return <NewTabPage />;
    case "settings":
      return <SettingsPage />;
    case "server":
      return <ServerView serverId={destination.serverId} />;
    case "console":
      return <ConsoleView serverId={destination.serverId} variant="dedicated" />;
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

export function App() {
  useEffect(() => {
    // Window housekeeping (ADR-0018) before the wire: the registry touch,
    // the prune of idle windows, and the one persist write that pins this
    // window's strip under its own key. The update lane boots beside it
    // (ADR-0024) — its cadence is its own; nothing here awaits it.
    bootWindow();
    startWire();
    void startUpdates();
  }, []);

  const tabs = useTabs((s) => s.tabs);
  const activeId = useTabs((s) => s.activeId);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  const newServerOpen = useUi((s) => s.newServerOpen);
  const paletteOpen = useUi((s) => s.paletteOpen);

  // The active tab, with the first tab as the honest fallback when the
  // stored id went stale.
  const active = useMemo(
    () => tabs.find((t) => t.id === activeId) ?? tabs[0],
    [tabs, activeId],
  );
  const destination = useMemo<Destination>(
    () => (active ? tabDestination(active) : { kind: "new" }),
    [active],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      // The browser keyboard contract (ADR-0032): Chromium's command
      // table (chrome_command_ids.h) decided in one pure function and
      // executed here against the stores. The destination model beneath
      // it stays ZaminPanel's — Ctrl+T opens a new-tab page (§6).
      const target = event.target as (HTMLElement & { isContentEditable: boolean }) | null;
      const api: BrowserKeyApi = {
        newTab: () => useTabs.getState().newTab(),
        reopenClosedTab: () => useTabs.getState().reopen(),
        closeActiveTab: () => {
          const id = useTabs.getState().activeId;
          if (id) useTabs.getState().close(id);
        },
        cycleTab: (step) => {
          const list = useTabs.getState().tabs;
          if (list.length < 2) return;
          const index = list.findIndex((t) => t.id === useTabs.getState().activeId);
          const next = list[(index + step + list.length) % list.length];
          if (next) useTabs.getState().setActive(next.id);
        },
        selectTabIndex: (index) => {
          const list = useTabs.getState().tabs;
          const tab = index === "last" ? list[list.length - 1] : list[index];
          if (tab) useTabs.getState().setActive(tab.id);
        },
        focusAddressBar: () =>
          window.dispatchEvent(new CustomEvent("zamin:focus-address")),
        reload: () => useTabs.getState().reload(),
        goBack: () => useTabs.getState().back(),
        goForward: () => useTabs.getState().forward(),
        bookmarkActive: () => {
          const store = useBookmarks.getState();
          const entries = Object.values(useServers.getState().servers);
          const current = tabs.find((t) => t.id === useTabs.getState().activeId);
          if (!current) return;
          const destination = tabDestination(current);
          if (isBookmarked(store.items, destination)) store.removeDestination(destination);
          else store.add(destination, destinationLabel(destination, entries));
        },
        toggleBookmarksBar: () => useBookmarks.getState().toggleBar(),
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
  }, [setPaletteOpen, tabs]);

  // The content key: the TAB's own id (two views of one server stay
  // isolated, §51), its destination, and a reload token — a new value
  // rebuilds the view, refreshes subscriptions, never touches the server
  // process (§60).
  const contentKey = `${active?.id ?? "none"}:${active ? tabKeyOf(active) : "none"}:${
    active?.reloadToken ?? 0
  }`;
  // §54: the strip's presentation is a window pref — vertical renders the
  // rail on the left and stacks the chrome beside it. The tab objects,
  // the mutations, and the identity rules are untouched.
  const vertical = useTabs((s) => s.verticalStrip);

  return (
    <div className={`${styles.shell} ${vertical ? styles.shellVertical : ""}`}>
      <TabStrip />
      <div className={styles.column}>
        <ToolBar />
        <BookmarksBar />
        <UpdateNotice />
        <main className={styles.content} key={contentKey}>
          <TabBoundary>
            <Suspense fallback={<div className={styles.lazyFallback} /> }>
              <DestinationView destination={destination} />
            </Suspense>
          </TabBoundary>
        </main>
      </div>
      <Suspense fallback={null}>
        {newServerOpen ? <NewServerModal /> : null}
        {paletteOpen ? <Palette /> : null}
        <ConnectionsModal />
      </Suspense>
    </div>
  );
}
