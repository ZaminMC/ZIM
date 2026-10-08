// Players: who is on the server right now, asked the way the workspace
// asks everything — the server itself (Server List Ping, no plugins).
// Refreshes on the architecture's 5–10 s cadence; an unreachable server
// is an empty room, not an error. Selecting a name opens the moderation
// cluster: ordinary console lines over the same stdin path the Console
// uses (state/moderation.ts) — the server executes, the log is the
// evidence, and the notes here never claim an outcome they cannot know.

import { useCallback, useEffect, useState } from "react";
import { listPlayers, sendStdin } from "../state/actions";
import type { PlayerSample, PlayersListResult } from "../protocol/types";
import { describeError } from "../state/errors";
import type { DescribedError } from "../state/errors";
import { ErrorNote } from "../ui/ErrorNote";
import {
  MODERATION_VERBS,
  moderationLine,
  type ModerationVerb,
} from "../state/moderation";
import { Button } from "../ui/Button";
import { useDeferredWindow } from "../ui/deferred";
import { StatusDot } from "../ui/StatusDot";
import styles from "./PlayersView.module.css";

const REFRESH_MS = 10_000;
/** How long the "sent" note stays before the row returns to rest. */
const SENT_NOTE_MS = 6_000;

// The deferred window resets on array identity; a fresh [] per render
// would reset it every time, so the empty fallback is a constant.
const NO_PLAYERS: PlayerSample[] = [];

export function PlayersView({
  serverId,
  running,
}: {
  serverId: string;
  running: boolean;
}) {
  const [result, setResult] = useState<PlayersListResult | null>(null);
  const [error, setError] = useState<DescribedError | null>(null);
  const [loading, setLoading] = useState(true);
  // The selected name (pills are buttons; keyboard selection is free).
  const [selected, setSelected] = useState<string | null>(null);
  // Ban confirms inline before it sends; the other verbs send at once.
  const [confirmingBan, setConfirmingBan] = useState(false);
  const [busy, setBusy] = useState(false);
  const [sentLine, setSentLine] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    setLoading(true);
    void listPlayers(serverId)
      .then((result) => {
        setResult(result);
        setError(null);
      })
      .catch((cause: unknown) => {
        setError(describeError(cause));
      })
      .finally(() => setLoading(false));
  }, [serverId]);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const select = (name: string) => {
    setSelected((current) => (current === name ? null : name));
    setConfirmingBan(false);
    setSentLine(null);
    setActionError(null);
  };

  const runVerb = (verb: ModerationVerb, name: string) => {
    const line = moderationLine(verb, name);
    if (line === null) {
      setActionError(
        `“${name}” is not a legal Minecraft username — nothing was sent.`,
      );
      return;
    }
    setBusy(true);
    setActionError(null);
    void sendStdin(serverId, line)
      .then(() => {
        setSentLine(line);
        setConfirmingBan(false);
        // The roster may answer the kick before this note expires; a
        // fresh ping right away keeps the two honest with each other.
        refresh();
        window.setTimeout(() => setSentLine(null), SENT_NOTE_MS);
      })
      .catch((cause: unknown) => {
        setActionError(describeError(cause).title);
      })
      .finally(() => setBusy(false));
  };

  const online = result?.online ?? null;
  // A busy server's log-derived roster can run long; it renders in slices
  // like the files table (see ui/deferred.ts). A fresh poll resets for free.
  const roster = useDeferredWindow(result?.roster ?? NO_PLAYERS, serverId);

  const pill = (name: string, key: string) => (
    <li key={key}>
      <button
        type="button"
        className={`${styles.player} ${selected === name ? styles.playerSelected : ""}`}
        aria-pressed={selected === name}
        onClick={() => select(name)}
      >
        {name}
      </button>
    </li>
  );

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
        <ErrorNote error={error} />
      </div>
      ) : null}

      {result && result.roster && result.roster.length > 0 ? (
        <div className={styles.rosterBlock}>
          <span className={styles.rosterLabel}>On right now — live from the log</span>
          <ul className={styles.names}>
            {roster.visible.map((player) => pill(player.name, `roster-${player.name}`))}
            {!roster.done ? (
              <li className={styles.more}>
                +{roster.pending} more rendering…{" "}
                <button className={styles.moreButton} onClick={roster.showAll}>
                  Show all
                </button>
              </li>
            ) : null}
          </ul>
        </div>
      ) : null}

      {result && result.sample.length > 0 ? (
        <div className={styles.rosterBlock}>
          {result.roster && result.roster.length > 0 ? (
            <span className={styles.rosterLabel}>The server's own status preview</span>
          ) : null}
          <ul className={styles.names}>
            {result.sample.map((player) =>
              pill(player.name, `sample-${player.id ?? player.name}`),
            )}
            {online !== null && online > result.sample.length ? (
              <li className={styles.more}>+{online - result.sample.length} more (server preview caps at 12)</li>
            ) : null}
          </ul>
        </div>
      ) : null}

      {selected ? (
        <div className={styles.moderation} role="region" aria-label={`Actions for ${selected}`}>
          <div className={styles.moderationHead}>
            <span className={styles.moderationName}>{selected}</span>
            <span className={styles.moderationHint}>
              {running
                ? "sends a console line — the server's answer lands in the log"
                : "the server must be running before a line can be sent"}
            </span>
          </div>
          <div className={styles.moderationActions}>
            {MODERATION_VERBS.map(({ verb, label, title, danger }) =>
              verb === "ban" && confirmingBan ? (
                <span key="ban-confirm" className={styles.confirm}>
                  <span>Ban “{selected}” until pardoned?</span>
                  <Button
                    variant="danger"
                    disabled={busy || !running}
                    onClick={() => runVerb("ban", selected)}
                  >
                    Yes, ban
                  </Button>
                  <Button disabled={busy} onClick={() => setConfirmingBan(false)}>
                    Keep
                  </Button>
                </span>
              ) : (
                <Button
                  key={verb}
                  variant={danger ? "danger" : "default"}
                  disabled={busy || !running}
                  title={title}
                  onClick={() =>
                    verb === "ban" ? setConfirmingBan(true) : runVerb(verb, selected)
                  }
                >
                  {label}
                </Button>
              ),
            )}
          </div>
          {sentLine ? (
            <p className={styles.sentNote} role="status">
              sent <code>{sentLine}</code> — the console's answer lands in the log
            </p>
          ) : null}
          {actionError ? (
            <p className={styles.actionError} role="alert">
              {actionError}
            </p>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
