// One open server: header (identity + lifecycle actions) + error surface
// + crash card + an icon tab row over the workspace surfaces (console,
// logs, files, players, schedules, backups). All outcomes arrive as events
// (ADR-0005); buttons only dispatch and wait, they never guess the
// resulting state.

import { lazy, Suspense, useEffect, useState } from "react";
import { getServer, killServer, restartServer, startServer, stopServer, writeWholeFile } from "../state/actions";
import { describeError } from "../state/errors";
import type { LifecycleVerb } from "../state/errors";
import type { ServerState } from "../protocol/types";
import { useServers } from "../state/servers";
import { useTabs } from "../state/tabs";
import { useUi } from "../state/ui";
import {
  ensureMetrics,
  releaseMetrics,
  useServerMetrics,
  formatBytes,
  formatCpu,
} from "../state/metrics";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/PromptDialog";
import { StatusChip } from "../ui/StatusChip";
import {
  IconActivity,
  IconBackups,
  IconBolt,
  IconClock,
  IconFolder,
  IconGear,
  IconLogs,
  IconNetwork,
  IconPlayers,
  IconPuzzle,
  IconRocket,
  IconServer,
  IconTerminal,
} from "../ui/icons";
import { CrashCard } from "./CrashCard";
import { ConsoleView } from "./ConsoleView";
import { LogViewer } from "./LogViewer";
import { MetricsView } from "./MetricsView";
import { PlayersView } from "./PlayersView";
import { PluginsView } from "./PluginsView";
import { PublishModal } from "./PublishModal";
import { SchedulesView } from "./SchedulesView";
import { BackupsView } from "./BackupsView";
import styles from "./ServerView.module.css";

// The three configuration surfaces (§37–39, ADR-0019) lazy-load: the
// workspace frame must not pay for forms a tab may never open. The
// console needs no lazy load since ADR-0020 — it is the structured view,
// with no heavy terminal dependency behind it.
const NetworkView = lazy(() =>
  import("./NetworkView").then((m) => ({ default: m.NetworkView })),
);
// The files browser (§32, ADR-0021) lazy-loads with the same reason: its
// editor, compose AST, and search walk are per-tab machinery a session
// may never open, and the cold start does not pay for them.
const FilesView = lazy(() =>
  import("./FilesView").then((m) => ({ default: m.FilesView })),
);
const StartupView = lazy(() =>
  import("./StartupView").then((m) => ({ default: m.StartupView })),
);
const SettingsView = lazy(() =>
  import("./SettingsView").then((m) => ({ default: m.SettingsView })),
);

/** Which verbs make sense from a given state (ADR-0005 ladder). */
export function availableVerbs(state: ServerState): LifecycleVerb[] {
  switch (state) {
    case "running":
      return ["stop", "restart", "kill"];
    case "starting":
    case "stopping":
    case "adopting":
      return ["stop", "restart", "kill"];
    case "not-running":
    case "stopped":
    case "failed-preflight":
    case "crashed":
    case "unknown":
      return ["start"];
  }
}

/** States where a server process may be alive and the sampler publishing. */
function isLiveState(state: ServerState): boolean {
  return (
    state === "running" || state === "starting" || state === "stopping" || state === "adopting"
  );
}

const LOWER_VIEWS = [
  { id: "console", label: "Console", icon: IconTerminal },
  { id: "metrics", label: "Metrics", icon: IconActivity },
  { id: "logs", label: "Logs", icon: IconLogs },
  { id: "files", label: "Files", icon: IconFolder },
  { id: "players", label: "Players", icon: IconPlayers },
  { id: "plugins", label: "Plugins", icon: IconPuzzle },
  { id: "schedules", label: "Schedules", icon: IconClock },
  { id: "network", label: "Network", icon: IconNetwork },
  { id: "startup", label: "Startup", icon: IconRocket },
  { id: "settings", label: "Settings", icon: IconGear },
  { id: "backups", label: "Backups", icon: IconBackups },
] as const;

type LowerView = (typeof LOWER_VIEWS)[number]["id"];

const VIEW_GROUPS: { label: string; ids: LowerView[] }[] = [
  { label: "Operate", ids: ["console", "metrics", "logs"] },
  { label: "Manage", ids: ["files", "players", "plugins"] },
  { label: "Automation", ids: ["schedules"] },
  { label: "System", ids: ["network", "startup", "settings", "backups"] },
];

/** Live sampler values in the header. Owns its store subscription so a
 *  1 Hz sample re-renders two chips, not the whole workspace (the panel
 *  budgets: interaction → next paint stays untouched by background data). */
function MetricsChips({ serverId, live }: { serverId: string; live: boolean }) {
  const samples = useServerMetrics(serverId);
  const latest = samples.at(-1);
  if (!live || !latest) return null;
  return (
    <div className={styles.metricChips}>
      <span className={styles.metricChip} title="CPU, sampled live">
        <IconBolt size={12} />
        {formatCpu(latest.cpuPercent)}
      </span>
      <span className={styles.metricChip} title="Resident memory">
        <IconActivity size={12} />
        {formatBytes(latest.rssBytes)}
      </span>
    </div>
  );
}

export function ServerView({ serverId }: { serverId: string }) {
  const server = useServers((s) => s.servers[serverId]);
  const upsert = useServers((s) => s.upsert);
  const pendingVerb = useUi((s) => s.pending[serverId]);
  const setPending = useUi((s) => s.setPending);
  const actionError = useUi((s) => s.actionErrors[serverId]);
  const setActionError = useUi((s) => s.setActionError);
  // Which lower surface the tab shows: the interactive console, the paged
  // log viewer, or the file browser. Panel-local, not persisted.
  const [lowerView, setLowerView] = useState<LowerView>("console");
  // The §24 publish affordance lives in the header's toolbar; the state
  // belongs to the whole server page, so it is declared with the page's
  // other hooks — before the unknown-server early return.
  const [publishOpen, setPublishOpen] = useState(false);
  const [killConfirmOpen, setKillConfirmOpen] = useState(false);

  // Details (software/version/port) arrive via server.get; refresh when the
  // server boots, since software identity is only knowable then.
  const state = server?.state;
  useEffect(() => {
    void getServer(serverId)
      .then((details) => upsert(details))
      .catch(() => {}); // the sidebar summary stays; errors surface on actions
  }, [serverId, state, upsert]);

  // The metrics stream lives for the whole workspace visit (header chips +
  // Metrics tab share it), reference-counted in state/metrics.ts.
  useEffect(() => {
    ensureMetrics(serverId);
    return () => releaseMetrics(serverId);
  }, [serverId]);

  if (!server) {
    // The gone-state is a quiet card, not a naked sentence in a corner:
    // one honest line, one way forward (the fleet page lists what the
    // daemon still sees).
    return (
      <div className={styles.view}>
        <div className={styles.goneCard} role="status">
          <span className={styles.goneIcon}>
            <IconServer size={20} />
          </span>
          <p className={styles.goneTitle}>This server is gone.</p>
          <p className={styles.goneBody}>
            It was removed from the registry — ZIM no longer knows it.
            The fleet page lists what ZIM still sees.
          </p>
          <Button
            variant="primary"
            onClick={() => useTabs.getState().navigate({ kind: "servers" })}
          >
            Back to the fleet
          </Button>
        </div>
      </div>
    );
  }

  const verbs = availableVerbs(server.state);

  const dispatch = (verb: LifecycleVerb) => {
    setActionError(serverId, null);
    setPending(serverId, verb);
    const run =
      verb === "start"
        ? startServer
        : verb === "stop"
          ? stopServer
          : verb === "restart"
            ? restartServer
            : killServer;
    void run(serverId)
      .catch((error: unknown) => {
        const described = describeError(error);
        setActionError(serverId, {
          code: described.code,
          message: described.title,
          remediation: described.remediation,
        });
      })
      .finally(() => setPending(serverId, null));
  };

  return (
    <div className={styles.view}>
      <header className={styles.header}>
        <div className={styles.headInfo}>
          <div className={styles.titleRow}>
            <h1 className={styles.name}>{server.displayName}</h1>
            <StatusChip state={server.state} />
          </div>
          <div className={styles.meta}>
            <span className={styles.codeChip}>{serverId}</span>
            {server.software ? <span className={styles.metaChip}>{server.software}</span> : null}
            {server.version ? <span className={styles.metaChip}>v{server.version}</span> : null}
            {server.port ? <span className={styles.metaChip}>:{server.port}</span> : null}
            <MetricsChips serverId={serverId} live={isLiveState(server.state)} />
          </div>
        </div>

        <div className={styles.actions} role="toolbar" aria-label="Server actions">
          <Button
            variant="primary"
            busy={pendingVerb === (isLiveState(server.state) ? "stop" : "start")}
            disabled={pendingVerb !== null && pendingVerb !== undefined || !verbs.includes(isLiveState(server.state) ? "stop" : "start")}
            onClick={() => dispatch(isLiveState(server.state) ? "stop" : "start")}
          >
            {isLiveState(server.state) ? "Stop" : "Start"}
          </Button>
          {verbs.includes("restart") ? (
            <Button
              variant="default"
              busy={pendingVerb === "restart"}
              disabled={pendingVerb !== null && pendingVerb !== undefined}
              onClick={() => dispatch("restart")}
            >
              Restart
            </Button>
          ) : null}
          {verbs.includes("kill") ? (
            <details className={styles.moreActions}>
              <summary aria-label="More server actions">More</summary>
              <div className={styles.actionMenu}>
                <Button
                  variant="danger"
                  className={styles.killAction}
                  busy={pendingVerb === "kill"}
                  disabled={pendingVerb !== null && pendingVerb !== undefined}
                  onClick={(event) => {
                    setKillConfirmOpen(true);
                    (event.currentTarget.closest("details") as HTMLDetailsElement | null)?.removeAttribute("open");
                  }}
                >
                  Kill server
                </Button>
              </div>
            </details>
          ) : null}
          <Button
            variant="default"
            data-testid="publish-open"
            disabled={pendingVerb !== null && pendingVerb !== undefined}
            onClick={() => setPublishOpen(true)}
            title="Package and publish a selection of this server's files"
          >
            Publish
          </Button>
        </div>
      </header>

      {actionError ? (
        <div className={styles.alert} role="alert">
          <div className={styles.alertHead}>
            <span className={styles.alertTitle}>{actionError.message}</span>
            <button
              className={styles.dismiss}
              onClick={() => setActionError(serverId, null)}
              aria-label="Dismiss error"
            >
              ×
            </button>
          </div>
          {actionError.code ? <span className={styles.codeChip}>{actionError.code}</span> : null}
          {actionError.code === "NEEDS_EULA" ? (
            <div className={styles.eulaRow}>
              <Button
                variant="primary"
                onClick={() => {
                  // The typed remediation, made one click wide: write the
                  // acceptance through the same rooted filesystem the
                  // daemon enforces, then retry the start.
                  setActionError(serverId, null);
                  void writeWholeFile(serverId, "eula.txt", new TextEncoder().encode("eula=true\n"))
                    .then(() => dispatch("start"))
                    .catch((error: unknown) => {
                      const described = describeError(error);
                      setActionError(serverId, {
                        code: described.code,
                        message: described.title,
                        remediation: described.remediation,
                      });
                    });
                }}
              >
                Accept EULA &amp; start
              </Button>
              <span className={styles.meta}>
                Accepting means you agree to Minecraft's EULA (aka.ms/minecrafteula).
              </span>
            </div>
          ) : null}
          {actionError.remediation.length > 0 ? (
            <ul className={styles.remediation}>
              {actionError.remediation.map((step) => (
                <li key={step}>{step}</li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}

      <CrashCard
        serverId={serverId}
        onViewLogs={() => setLowerView("logs")}
        onRecover={() => setLowerView("backups")}
      />

      <div className={styles.workspace}>
        <nav className={styles.viewSwitch} aria-label="Server workspace">
          {VIEW_GROUPS.map((group) => (
            <div className={styles.viewGroup} key={group.label}>
              <h2 className={styles.viewGroupTitle}>{group.label}</h2>
              <div className={styles.viewGroupItems}>
                {LOWER_VIEWS.filter(({ id }) => group.ids.includes(id)).map(({ id, label, icon: Icon }) => (
                  <button
                    key={id}
                    type="button"
                    aria-current={lowerView === id ? "page" : undefined}
                    className={`${styles.viewTab} ${lowerView === id ? styles.viewTabActive : ""}`}
                    onClick={() => setLowerView(id)}
                  >
                    <Icon size={16} />
                    <span>{label}</span>
                  </button>
                ))}
              </div>
            </div>
          ))}
        </nav>

      <div className={styles.panel}>
        <Suspense fallback={<div className={styles.lazyLoad} aria-busy="true" />}>
          {lowerView === "console" ? (
            <ConsoleView
              serverId={serverId}
              onOpenInNewTab={() =>
                useTabs.getState().navigate({ kind: "console", serverId })
              }
            />
          ) : lowerView === "metrics" ? (
            <MetricsView serverId={serverId} />
          ) : lowerView === "logs" ? (
            <LogViewer serverId={serverId} />
          ) : lowerView === "files" ? (
            <FilesView serverId={serverId} />
          ) : lowerView === "players" ? (
            <PlayersView serverId={serverId} running={server.state === "running"} />
          ) : lowerView === "plugins" ? (
            <PluginsView serverId={serverId} />
          ) : lowerView === "schedules" ? (
            <SchedulesView serverId={serverId} />
          ) : lowerView === "network" ? (
            <NetworkView serverId={serverId} />
          ) : lowerView === "startup" ? (
            <StartupView serverId={serverId} />
          ) : lowerView === "settings" ? (
            <SettingsView serverId={serverId} />
          ) : (
            <BackupsView serverId={serverId} running={server.state === "running"} />
          )}
        </Suspense>
        </div>
      </div>

      {publishOpen ? (
        <PublishModal
          serverId={serverId}
          serverName={server.displayName}
          onClose={() => setPublishOpen(false)}
        />
      ) : null}

      {killConfirmOpen ? (
        <ConfirmDialog
          title={`Force-kill ${server.displayName}?`}
          body="This immediately terminates the server process. Unsaved server data may be lost."
          confirmLabel="Kill server"
          danger
          onConfirm={() => dispatch("kill")}
          onClose={() => setKillConfirmOpen(false)}
        />
      ) : null}
    </div>
  );
}
