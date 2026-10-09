// zim://audit/ — the audit page (§72's read side, ADR-0011,
// ADR-0026). The daemon appends one JSONL line per handshake and per
// mutating command; this page reads the trail back newest-first, paged,
// and honest about the lines that never parsed (they are counted, never
// dropped, never rewritten). Reads are not audited — a listing floods
// the file without making anything safer — so this page never appears
// in the trail it reads.

import { useEffect, useState } from "react";
import { listAudit } from "../state/actions";
import { describeError, type DescribedError } from "../state/errors";
import { useServers } from "../state/servers";
import type { AuditEntry } from "../protocol/types";
import { Button } from "../ui/Button";
import { ErrorNote } from "../ui/ErrorNote";
import styles from "./AuditPage.module.css";

const PAGE_SIZE = 100;

function formatWhen(tsMs: number): string {
  const date = new Date(tsMs);
  return Number.isNaN(date.getTime()) ? "—" : date.toLocaleString();
}

function OutcomeChip({ outcome }: { outcome: string }) {
  const ok = outcome === "ok";
  return (
    <span className={`${styles.outcome} ${ok ? styles.outcomeOk : styles.outcomeError}`}>
      {ok ? "ok" : outcome}
    </span>
  );
}

function AuditRow({ entry, serverName }: { entry: AuditEntry; serverName?: string }) {
  return (
    <li className={styles.row}>
      <span className={styles.when}>{formatWhen(entry.tsMs)}</span>
      <span className={styles.method}>{entry.method}</span>
      <span className={styles.server}>{serverName ?? entry.serverId ?? ""}</span>
      <OutcomeChip outcome={entry.outcome} />
      <span className={styles.client}>
        {entry.client ? `${entry.client.name} v${entry.client.version}` : ""}
      </span>
    </li>
  );
}

export function AuditPage() {
  const servers = useServers((s) => s.servers);
  const [entries, setEntries] = useState<AuditEntry[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [malformed, setMalformed] = useState(0);
  const [error, setError] = useState<DescribedError | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingOlder, setLoadingOlder] = useState(false);

  const load = (offset: number, append: boolean) => {
    if (append) setLoadingOlder(true);
    else setLoading(true);
    listAudit({ limit: PAGE_SIZE, offset })
      .then((result) => {
        setEntries((prev) => (append ? [...prev, ...result.entries] : result.entries));
        setHasMore(result.hasMore);
        setMalformed(result.malformed);
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause)))
      .finally(() => {
        setLoading(false);
        setLoadingOlder(false);
      });
  };

  useEffect(() => {
    load(0, false);
  }, []);

  const serverName = (serverId: string | undefined): string | undefined =>
    serverId ? (servers[serverId]?.displayName ?? serverId) : undefined;

  return (
    <div className={styles.page}>
      <header className={styles.pageHead}>
        <div>
          <h1 className={styles.title}>Audit log</h1>
          <p className={styles.subtitle}>
            Every mutating command and handshake, newest first — appended by the daemon, read
            only here.
          </p>
        </div>
        <Button variant="ghost" onClick={() => load(0, false)} disabled={loading}>
          Refresh
        </Button>
      </header>

      {error ? (
        <div className={styles.alert} role="alert">
          <ErrorNote error={error} />
        </div>
      ) : null}

      {entries.length > 0 ? (
        <>
          <div className={styles.tableHead} aria-hidden>
            <span>When</span>
            <span>Command</span>
            <span>Server</span>
            <span>Outcome</span>
            <span>Client</span>
          </div>
          <ul className={styles.list}>
            {entries.map((entry, index) => (
              <AuditRow
                key={`${entry.tsMs}-${entry.method}-${index}`}
                entry={entry}
                serverName={serverName(entry.serverId)}
              />
            ))}
          </ul>
          {hasMore ? (
            <div className={styles.more}>
              <Button
                variant="ghost"
                onClick={() => load(entries.length, true)}
                disabled={loadingOlder}
              >
                {loadingOlder ? "Reading…" : "Load older entries"}
              </Button>
            </div>
          ) : null}
        </>
      ) : loading ? (
        <p className={styles.empty}>Reading the audit trail…</p>
      ) : error ? null : (
        <p className={styles.empty}>
          Nothing audited yet — the trail fills as commands and connections land.
        </p>
      )}

      {malformed > 0 ? (
        <p className={styles.malformed} role="status">
          {malformed} {malformed === 1 ? "line" : "lines"} on disk did not parse as audit JSON.
          {malformed === 1 ? " It" : " They"} {malformed === 1 ? "stays" : "stay"} on disk,
          counted here rather than hidden.
        </p>
      ) : null}

      <p className={styles.note}>
        Reads are not audited: a listing would flood the trail without making anything safer.
        Every mutating command appears after the daemon has answered it, with the outcome the
        daemon gave.
      </p>
    </div>
  );
}

