// Players: who is on the server right now, asked the way the workspace
// asks everything — the server itself (Server List Ping, no plugins).
// Refreshes on the architecture's 5–10 s cadence; an unreachable server
// is an empty room, not an error.

import { useCallback, useEffect, useState } from "react";
import { listPlayers } from "../state/actions";
import type { PlayersListResult } from "../protocol/types";
import { describeError } from "../state/errors";
import { Button } from "../ui/Button";
import { StatusDot } from "../ui/StatusDot";
import styles from "./PlayersView.module.css";

const REFRESH_MS = 10_000;

export function PlayersView({ serverId }: { serverId: string }) {
  const [result, setResult] = useState<PlayersListResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(() => {
    setLoading(true);
    void listPlayers(serverId)
      .then((result) => {
        setResult(result);
        setError(null);
      })
      .catch((cause: unknown) => {
        setError(describeError(cause).title);
      })
      .finally(() => setLoading(false));
  }, [serverId]);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const online = result?.online ?? null;

  return (
    <section className={styles.players} aria-label="Players">
      <div className={styles.head}>
        <StatusDot state={online === null ? "unknown" : online > 0 ? "running" : "stopped"} />
        <span className={styles.count}>
          {online === null
            ? "nobody — the server is not answering pings"
            : `${online} of ${result?.max ?? "?"} players online`}
        </span>
        {result ? <span className={styles.latency}>{result.latencyMs} ms</span> : null}
        <span className={styles.spacer} />
        <Button onClick={refresh} disabled={loading}>
          {loading ? "Pinging…" : "Refresh"}
        </Button>
      </div>

      {result?.motd ? <p className={styles.motd}>{result.motd}</p> : null}
      {result?.version ? (
        <p className={styles.version}>server version {result.version}</p>
      ) : null}
      {error ? (
        <div className={styles.alert} role="alert">
          {error}
        </div>
      ) : null}

      {result && result.sample.length > 0 ? (
        <ul className={styles.names}>
          {result.sample.map((player) => (
            <li key={player.id ?? player.name} className={styles.player}>
              {player.name}
            </li>
          ))}
          {online !== null && online > result.sample.length ? (
            <li className={styles.more}>+{online - result.sample.length} more (server preview caps at 12)</li>
          ) : null}
        </ul>
      ) : null}
    </section>
  );
}
