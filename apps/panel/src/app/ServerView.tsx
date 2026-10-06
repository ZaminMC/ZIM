// One open server: header + lifecycle actions + error surface + crash card
// + a view switch between the console (interactive terminal) and the log
// viewer (paged, filterable history + live tail). All outcomes arrive as
// events (ADR-0005); buttons only dispatch and wait, they never guess the
// resulting state.

import { useEffect, useState } from "react";
import { getServer, killServer, restartServer, startServer, stopServer, writeWholeFile } from "../state/actions";
import { describeError, LIFECYCLE_VERBS } from "../state/errors";
import type { LifecycleVerb } from "../state/errors";
import type { ServerState } from "../protocol/types";
import { useServers } from "../state/servers";
import { useUi } from "../state/ui";
import { Button } from "../ui/Button";
import { StatusDot } from "../ui/StatusDot";
import { Console } from "./Console";
import { CrashCard } from "./CrashCard";
import { FilesView } from "./FilesView";
import { LogViewer } from "./LogViewer";
import { PlayersView } from "./PlayersView";
import { BackupsView } from "./BackupsView";
import styles from "./ServerView.module.css";

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

export function ServerView({ serverId }: { serverId: string }) {
  const server = useServers((s) => s.servers[serverId]);
  const upsert = useServers((s) => s.upsert);
  const pendingVerb = useUi((s) => s.pending[serverId]);
  const setPending = useUi((s) => s.setPending);
  const actionError = useUi((s) => s.actionErrors[serverId]);
  const setActionError = useUi((s) => s.setActionError);
  // Which lower surface the tab shows: the interactive console, the paged
  // log viewer, or the file browser. Panel-local, not persisted.
  const [lowerView, setLowerView] = useState<
    "console" | "logs" | "files" | "players" | "backups"
  >("console");

  // Details (software/version/port) arrive via server.get; refresh when the
  // server boots, since software identity is only knowable then.
  const state = server?.state;
  useEffect(() => {
    void getServer(serverId)
      .then((details) => upsert(details))
      .catch(() => {}); // the sidebar summary stays; errors surface on actions
  }, [serverId, state, upsert]);

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
        <StatusDot state={server.state} />
        <h1 className={styles.name}>{server.displayName}</h1>
        <div className={styles.meta}>
          <span className={styles.codeChip}>{serverId}</span>
          {server.software ? <span className={styles.metaChip}>{server.software}</span> : null}
          {server.version ? <span className={styles.metaChip}>v{server.version}</span> : null}
          {server.port ? <span className={styles.metaChip}>:{server.port}</span> : null}
        </div>
      </header>

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
      </div>

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
        <button
          role="tab"
          aria-selected={lowerView === "console"}
          className={`${styles.viewTab} ${lowerView === "console" ? styles.viewTabActive : ""}`}
          onClick={() => setLowerView("console")}
        >
          Console
        </button>
        <button
          role="tab"
          aria-selected={lowerView === "logs"}
          className={`${styles.viewTab} ${lowerView === "logs" ? styles.viewTabActive : ""}`}
          onClick={() => setLowerView("logs")}
        >
          Logs
        </button>
        <button
          role="tab"
          aria-selected={lowerView === "files"}
          className={`${styles.viewTab} ${lowerView === "files" ? styles.viewTabActive : ""}`}
          onClick={() => setLowerView("files")}
        >
          Files
        </button>
        <button
          role="tab"
          aria-selected={lowerView === "players"}
          className={`${styles.viewTab} ${lowerView === "players" ? styles.viewTabActive : ""}`}
          onClick={() => setLowerView("players")}
        >
          Players
        </button>
        <button
          role="tab"
          aria-selected={lowerView === "backups"}
          className={`${styles.viewTab} ${lowerView === "backups" ? styles.viewTabActive : ""}`}
          onClick={() => setLowerView("backups")}
        >
          Backups
        </button>
      </div>
      {lowerView === "console" ? (
        <Console serverId={serverId} running={server.state === "running"} />
      ) : lowerView === "logs" ? (
        <LogViewer serverId={serverId} />
      ) : lowerView === "files" ? (
        <FilesView serverId={serverId} />
      ) : lowerView === "players" ? (
        <PlayersView serverId={serverId} />
      ) : (
        <BackupsView serverId={serverId} running={server.state === "running"} />
      )}
    </div>
  );
}
