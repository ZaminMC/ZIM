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

/** The audit protocol pages newest-first by an offset counted back from
 *  the NEWEST line — a basis that SHIFTS by one every time the daemon
 *  appends. Paging by "rows already held" (the old offset arithmetic)
 *  re-delivers rows that shifted into the next page's window: duplicates
 *  whenever anything was audited between clicks. The honest fix inside
 *  the protocol as it stands: page by DEPTH — each "load older" re-reads
 *  one consistent snapshot `depth + PAGE_SIZE` rows deep from the current
 *  newest. No row can duplicate (one basis, one read), new lines simply
 *  appear at the top (they were audited; the operator should see them),
 *  and the cost is one bounded re-read of rows the page already priced. */
const DEEPEST_SNAPSHOT = 2000;

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

  const load = (depth: number) => {
    const limit = Math.min(depth, DEEPEST_SNAPSHOT);
    setLoading(true);
    listAudit({ limit, offset: 0 })
      .then((result) => {
        setEntries(result.entries);
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

  // The view's current depth: grows by one page per "load older", resets
  // to the first page on mount.
  const [depth, setDepth] = useState(PAGE_SIZE);

  useEffect(() => {
    load(PAGE_SIZE);
  }, []);

  const serverName = (serverId: string | undefined): string | undefined =>
    serverId ? (servers[serverId]?.displayName ?? serverId) : undefined;

  return (
    <div className={styles.page}>
      <header className={styles.pageHead}>
        <div>
          <h1 className={styles.title}>Audit log</h1>
          <p className={styles.subtitle}>
            Every changing action and connection event, newest first — written by ZIM's service,
            read only here.
          </p>
        </div>
        <Button variant="ghost" onClick={() => load(depth)} disabled={loading}>
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
                onClick={() => {
                  setLoadingOlder(true);
                  const next = Math.min(depth + PAGE_SIZE, DEEPEST_SNAPSHOT);
                  setDepth(next);
                  load(next);
                }}
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
        Every changing action appears here once ZIM's service has answered it, with the outcome
        that answer gave.
      </p>
    </div>
  );
}

