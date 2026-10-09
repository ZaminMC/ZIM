// The Join page (§7's completion): a typed join address lands here —
// never in a webview navigation. The daemon classifies the address
// (registry + server-list ping) and this view speaks the verdict with
// its recovery paths. A Minecraft server browser answers like one.

import { useCallback, useEffect, useState } from "react";
import { checkJoin, startServer } from "../../state/actions";
import type { JoinCheckResult } from "../../protocol/types";
import { describeError, type DescribedError } from "../../state/errors";
import { useConnection } from "../../state/connection";
import { useServers, type ServerEntry } from "../../state/servers";
import { useUi } from "../../state/ui";
import { navigateHost } from "../../state/shellLane";
import { invokeIfTauri } from "../../integration/tauri";
import { Button } from "../../ui/Button";
import { ErrorNote } from "../../ui/ErrorNote";
import styles from "./JoinPage.module.css";

/** The registry's honest overlap: a managed server whose port is the
 *  address's port. The host part is advisory (the registry stores
 *  directories, not addresses) — a port match alone is reported as
 *  "uses this port", never as "this is the server you asked for". */
function registryMatch(port: number, entries: ServerEntry[]): ServerEntry | undefined {
  return entries.find((entry) => entry.port === port);
}

interface Verdict {
  title: string;
  body: string;
  tone: "down" | "unreachable" | "invalid" | "alive";
}

export function verdictFor(result: JoinCheckResult, address: string, stopped?: ServerEntry): Verdict {
  switch (result.state) {
    case "alive":
      return {
        title: "This server is online",
        body: `${address} answered the Minecraft server-list ping.`,
        tone: "alive",
      };
    case "refused":
      return stopped
        ? {
            title: `Your server is stopped`,
            body: `Nothing is listening on port ${stopped.port} because ${stopped.displayName} is not running.`,
            tone: "down",
          }
        : {
            title: `No Server Running on Port ${address.split(":").pop()}`,
            body: `Could not establish a connection to ${address}. Nothing is listening on that port.`,
            tone: "down",
          };
    case "timeout":
      return {
        title: "The server did not answer",
        body: `${address} accepted the connection but never completed the Minecraft handshake. It may be overloaded, still starting, or running something that is not a Minecraft server.`,
        tone: "down",
      };
    case "unreachable":
      return {
        title: `Can’t reach ${address.split(":")[0]}`,
        body: `No route to ${address}. The machine may be offline, the address mistyped, or the network refusing the route.`,
        tone: "unreachable",
      };
    case "invalid":
      return {
        title: "That address is not a Minecraft server",
        body: `${address} answered, but not in the Minecraft server protocol. Check the address and the port.`,
        tone: "invalid",
      };
  }
}

export function JoinPage({ host, port }: { host?: string; port: number }) {
  const address = host ? `${host}:${port}` : `localhost:${port}`;
  const status = useConnection((s) => s.status);
  const entries = useServers((s) => s.servers);
  const setNewServerOpen = useUi((s) => s.setNewServerOpen);
  const setNewServerPort = useUi((s) => s.setNewServerPort);

  const [phase, setPhase] = useState<
    { kind: "loading" } | { kind: "result"; result: JoinCheckResult } | { kind: "error"; note: DescribedError }
  >({ kind: "loading" });
  // A retry bumps the epoch; the effect re-runs and re-checks.
  const [epoch, setEpoch] = useState(0);

  const stopped = registryMatch(port, Object.values(entries));

  const run = useCallback(() => {
    setPhase({ kind: "loading" });
    checkJoin(host, port)
      .then((result) => setPhase({ kind: "result", result }))
      .catch((error: unknown) => setPhase({ kind: "error", note: describeError(error) }));
  }, [host, port]);

  useEffect(() => {
    if (status !== "ready") {
      setPhase({ kind: "loading" });
      return;
    }
    run();
  }, [status, run, epoch]);

  const back = () => {
    // The tab's own history entry back out of the join attempt (§59).
    void invokeIfTauri("shell_tab_action", { action: "back" });
  };

  if (status !== "ready") {
    return (
      <div className={styles.page}>
        <div className={styles.card}>
          <h1 className={styles.title}>The daemon is not answering</h1>
          <p className={styles.body}>
            The verdict on <code className={styles.url}>{address}</code> needs the daemon, and ZIM
            is reconnecting to it right now. Nothing was checked yet.
          </p>
          <div className={styles.actions}>
            <Button variant="primary" onClick={() => setEpoch((n) => n + 1)}>
              Retry
            </Button>
            <Button onClick={back}>Back</Button>
          </div>
        </div>
      </div>
    );
  }

  if (phase.kind === "loading") {
    return (
      <div className={styles.page}>
        <div className={styles.card}>
          <h1 className={styles.title}>Checking {address}…</h1>
          <p className={styles.body}>
            ZIM is asking the daemon what lives at this address — the registry first, then the
            Minecraft server-list ping.
          </p>
        </div>
      </div>
    );
  }

  if (phase.kind === "error") {
    return (
      <div className={styles.page}>
        <div className={styles.card}>
          <ErrorNote error={phase.note} />
          <div className={styles.actions}>
            <Button variant="primary" onClick={() => setEpoch((n) => n + 1)}>
              Retry
            </Button>
            <Button onClick={back}>Back</Button>
          </div>
        </div>
      </div>
    );
  }

  const verdict = verdictFor(phase.result, address, stopped);
  const result = phase.result;

  return (
    <div className={styles.page}>
      <div className={`${styles.card} ${styles[verdict.tone]}`}>
        <h1 className={styles.title}>{verdict.title}</h1>
        <p className={styles.body}>{verdict.body}</p>

        {result.state === "alive" ? (
          <dl className={styles.facts}>
            {result.version ? (
              <>
                <dt>Version</dt>
                <dd>{result.version}</dd>
              </>
            ) : null}
            {result.playersMax != null ? (
              <>
                <dt>Players</dt>
                <dd>
                  {result.playersOnline ?? 0} / {result.playersMax}
                </dd>
              </>
            ) : null}
            {result.motd ? (
              <>
                <dt>MOTD</dt>
                <dd>{result.motd}</dd>
              </>
            ) : null}
            <dt>Address</dt>
            <dd>
              <code className={styles.url}>{address}</code>
            </dd>
          </dl>
        ) : null}

        {result.state === "refused" && stopped ? (
          <div className={styles.stopped}>
            <p className={styles.body}>
              {stopped.displayName} ({stopped.serverId}) uses this port. Start it, then join from
              its page.
            </p>
            <div className={styles.actions}>
              <Button
                variant="primary"
                onClick={() => {
                  void startServer(stopped.serverId).catch(() => {});
                  void navigateHost({ kind: "server", serverId: stopped.serverId });
                }}
              >
                Start {stopped.displayName}
              </Button>
              <Button onClick={() => void navigateHost({ kind: "server", serverId: stopped.serverId })}>
                Open server
              </Button>
            </div>
          </div>
        ) : null}

        {result.state === "refused" && !stopped ? (
          <div className={styles.actions}>
            <Button
              variant="primary"
              onClick={() => {
                setNewServerPort(String(port));
                setNewServerOpen(true);
              }}
            >
              Create server on this port
            </Button>
            <Button onClick={() => setEpoch((n) => n + 1)}>Retry</Button>
            <Button onClick={back}>Back</Button>
          </div>
        ) : null}

        {result.state !== "refused" ? (
          <div className={styles.actions}>
            <Button variant="primary" onClick={() => setEpoch((n) => n + 1)}>
              Retry
            </Button>
            <Button onClick={back}>Back</Button>
          </div>
        ) : null}
      </div>
    </div>
  );
}
