// One open server: header (identity + lifecycle actions) + error surface
// + crash card + an icon tab row over the workspace surfaces (console,
// logs, files, players, schedules, backups). All outcomes arrive as events
// (ADR-0005); buttons only dispatch and wait, they never guess the
// resulting state.

import { lazy, Suspense, useEffect, useState } from "react";
import { getServer, killServer, restartServer, startServer, stopServer, writeWholeFile } from "../state/actions";
import { describeError, LIFECYCLE_VERBS } from "../state/errors";
import type { LifecycleVerb } from "../state/errors";
import type { ServerState } from "../protocol/types";
import { useServers } from "../state/servers";
import { useUi } from "../state/ui";
import {
  ensureMetrics,
  releaseMetrics,
  useServerMetrics,
  formatBytes,
  formatCpu,
} from "../state/metrics";
import { Button } from "../ui/Button";
import { StatusChip } from "../ui/StatusChip";
import {
  IconActivity,
  IconBackups,
  IconBolt,
  IconClock,
  IconFolder,
  IconLogs,
  IconPlayers,
  IconPuzzle,
  IconTerminal,
} from "../ui/icons";
import { CrashCard } from "./CrashCard";
import { FilesView } from "./FilesView";
import { LogViewer } from "./LogViewer";
import { MetricsView } from "./MetricsView";
import { PlayersView } from "./PlayersView";
import { PluginsView } from "./PluginsView";
import { PublishModal } from "./PublishModal";
import { SchedulesView } from "./SchedulesView";
import { BackupsView } from "./BackupsView";
import styles from "./ServerView.module.css";

// xterm (+ addons) is the panel's heaviest dependency and only the console
// tab needs it: it loads when the tab first renders, so the cold start
// ships without it (PERFORMANCE-BUDGETS: cold start → interactive).
const Console = lazy(() => import("./Console").then((m) => ({ default: m.Console })));

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
  { id: "backups", label: "Backups", icon: IconBackups },
] as const;

type LowerView = (typeof LOWER_VIEWS)[number]["id"];

/** Live sampler values in the header. Owns its store subscription so a
 *  1 Hz sample re-renders two chips, not the whole workspace (the panel
 *  budgets: interaction → next paint stays untouched by background data). */
function MetricsChips({ serverId, live }: { serverId: string; live: boolean }) {
  const samples = useServerMetrics(serverId);
  const latest = samples.at(-1);
  if (!live || !latest) return null;
  return (
    <div className={styles.metricChips}>
      <span className={styles.metricChip} title="CPU, from the daemon sampler">
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
    return (
      <div className={styles.view}>
        <p className={styles.meta}>This server is gone (removed from the registry).</p>
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
          {LIFECYCLE_VERBS.map((verb) => {
            const allowed = verbs.includes(verb);
            return (
              <Button
                key={verb}
                variant={verb === "start" ? "primary" : verb === "kill" ? "danger" : "default"}
                disabled={!allowed}
                busy={pendingVerb === verb}
                onClick={() => dispatch(verb)}
                title={allowed ? undefined : `${verb} is not available while ${server.state}`}
              >
                {verb.charAt(0).toUpperCase() + verb.slice(1)}
              </Button>
            );
          })}
          {/* §24 reserves this slot: the primary publish action, always
              reachable — the workspace opens over the server page. */}
          <Button
            variant="default"
            data-testid="publish-open"
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
        onRecover={() => setLowerView("backups")}
      />

      <div className={styles.viewSwitch} role="tablist" aria-label="Output view">
        {LOWER_VIEWS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            role="tab"
            aria-selected={lowerView === id}
            className={`${styles.viewTab} ${lowerView === id ? styles.viewTabActive : ""}`}
            onClick={() => setLowerView(id)}
          >
            <Icon size={15} />
            {label}
          </button>
        ))}
      </div>

      <div className={styles.panel}>
        <Suspense fallback={<div className={styles.lazyLoad} aria-busy="true" />}>
          {lowerView === "console" ? (
            <Console serverId={serverId} running={server.state === "running"} />
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
          ) : (
            <BackupsView serverId={serverId} running={server.state === "running"} />
          )}
        </Suspense>
      </div>

      {publishOpen ? (
        <PublishModal
          serverId={serverId}
          serverName={server.displayName}
          onClose={() => setPublishOpen(false)}
        />
      ) : null}
    </div>
  );
}
