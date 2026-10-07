// The browser shell (ADR-0015): a tab strip, a tool bar whose address bar
// speaks the product's dialects, and one destination at a time. The
// sidebar is gone — ZaminPanel is a browser for Minecraft servers, and
// §84's first slice lives here. Every view below the chrome is the
// workspace that already existed; the frame is what changed.

import { Suspense, lazy, useEffect, useMemo } from "react";
import type { Destination } from "../state/destinations";
import { sortedServers, useServers } from "../state/servers";
import { tabDestination, tabKeyOf, useTabs } from "../state/tabs";
import { useUi } from "../state/ui";
import { startWire } from "../state/wire";
import { ServerView } from "./ServerView";
import { FleetPage } from "./FleetPage";
import { MissingPage } from "./browser/MissingPage";
import { NewTabPage } from "./browser/NewTabPage";
import { SettingsPage } from "./browser/SettingsPage";
import { TabStrip } from "./browser/TabStrip";
import { ToolBar } from "./browser/ToolBar";
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
    case "missing":
      return <MissingPage url={destination.url} />;
  }
}

export function App() {
  useEffect(() => {
    startWire();
  }, []);

  const tabs = useTabs((s) => s.tabs);
  const activeKey = useTabs((s) => s.activeKey);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  const newServerOpen = useUi((s) => s.newServerOpen);
  const paletteOpen = useUi((s) => s.paletteOpen);

  // The active tab, with the first tab as the honest fallback when the
  // stored key went stale.
  const active = useMemo(
    () => tabs.find((t) => tabKeyOf(t) === activeKey) ?? tabs[0],
    [tabs, activeKey],
  );
  const destination = useMemo<Destination>(
    () => (active ? tabDestination(active) : { kind: "new" }),
    [active],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      const tabsApi = useTabs.getState();
      if (mod && !event.shiftKey && event.key.toLowerCase() === "k") {
        // The palette keeps its §53 journey.
        event.preventDefault();
        setPaletteOpen(!useUi.getState().paletteOpen);
        return;
      }
      if (mod && event.key.toLowerCase() === "t") {
        event.preventDefault();
        tabsApi.newTab();
        return;
      }
      if (mod && event.key.toLowerCase() === "w") {
        event.preventDefault();
        const key = useTabs.getState().activeKey;
        if (key) tabsApi.close(key);
        return;
      }
      if (mod && event.key.toLowerCase() === "l") {
        event.preventDefault();
        window.dispatchEvent(new CustomEvent("zamin:focus-address"));
        return;
      }
      if (event.key === "F6") {
        event.preventDefault();
        window.dispatchEvent(new CustomEvent("zamin:focus-address"));
        return;
      }
      if (mod && event.key.toLowerCase() === "r") {
        event.preventDefault();
        tabsApi.reload();
        return;
      }
      if (event.altKey && event.key === "ArrowLeft") {
        event.preventDefault();
        tabsApi.back();
        return;
      }
      if (event.altKey && event.key === "ArrowRight") {
        event.preventDefault();
        tabsApi.forward();
        return;
      }
      if (mod && event.key === "Tab") {
        event.preventDefault();
        const list = useTabs.getState().tabs;
        if (list.length < 2) return;
        const currentKey = useTabs.getState().activeKey;
        const index = list.findIndex((t) => tabKeyOf(t) === currentKey);
        const step = event.shiftKey ? -1 : 1;
        const next = list[(index + step + list.length) % list.length];
        if (next) tabsApi.setActive(tabKeyOf(next));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setPaletteOpen]);

  // The content key: a new destination or a reload token rebuilds the
  // view — refresh state, reconnect subscriptions, never touch the
  // server process (§60).
  const contentKey = `${active ? tabKeyOf(active) : "none"}:${active?.reloadToken ?? 0}`;

  return (
    <div className={styles.shell}>
      <TabStrip />
      <ToolBar />
      <main className={styles.content} key={contentKey}>
        <DestinationView destination={destination} />
      </main>
      <Suspense fallback={null}>
        {newServerOpen ? <NewServerModal /> : null}
        {paletteOpen ? <Palette /> : null}
        <ConnectionsModal />
      </Suspense>
    </div>
  );
}
