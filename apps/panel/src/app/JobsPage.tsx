// zim://jobs/ — the jobs page (§73, ADR-0026). Every long-running
// daemon operation — backups, restores, publishes, plugin and Java
// installs — is a Job with an id, a state, a progress, and a cancel verb,
// and the daemon is the authoritative record: it keeps the newest 50
// finished jobs, survives reconnections, and publishes every transition
// on the events stream. This page seeds from `jobs.list` on mount and
// then rides the live events through the jobs store; it never invents a
// job, a percentage, or a completion (§82).

import { useEffect, useState } from "react";
import { cancelJob, listJobs } from "../state/actions";
import { describeError, type DescribedError } from "../state/errors";
import { useJobs } from "../state/jobs";
import { useServers } from "../state/servers";
import type { Job } from "../protocol/types";
import { Button } from "../ui/Button";
import { ErrorNote } from "../ui/ErrorNote";
import styles from "./JobsPage.module.css";

const STATE_LABEL: Record<Job["state"], string> = {
  queued: "Queued",
  running: "Running",
  succeeded: "Succeeded",
  failed: "Failed",
  cancelled: "Cancelled",
};

function formatWhen(ms: number | undefined): string {
  if (ms === undefined) return "—";
  const date = new Date(ms);
  return Number.isNaN(date.getTime()) ? "—" : date.toLocaleString();
}

function JobRow({ job }: { job: Job }) {
  const servers = useServers((s) => s.servers);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<DescribedError | null>(null);
  const cancellable = job.state === "queued" || job.state === "running";
  const serverName = job.serverId
    ? (servers[job.serverId]?.displayName ?? job.serverId)
    : undefined;
  const progress = job.progress;
  const share =
    progress && progress.total !== undefined && progress.total > 0
      ? Math.min(100, Math.round((progress.current / progress.total) * 100))
      : null;

  const cancel = () => {
    if (!cancellable || busy) return;
    setBusy(true);
    setActionError(null);
    // The cancellation is a request: the state flip to `cancelled` is the
    // daemon's, made when the task observes the flag at its checkpoint —
    // the row changes when the event says so, not when the click lands.
    cancelJob(job.jobId)
      .catch((cause: unknown) => setActionError(describeError(cause)))
      .finally(() => setBusy(false));
  };

  return (
    <li className={styles.row} data-state={job.state}>
      <div className={styles.rowMain}>
        <span className={styles.kind}>{job.kind}</span>
        <span className={styles.jobId} title={job.jobId}>
          {job.jobId}
        </span>
        {serverName ? <span className={styles.server}>{serverName}</span> : null}
        <span className={`${styles.state} ${styles[`state_${job.state}`]}`}>
          {STATE_LABEL[job.state]}
        </span>
        {cancellable ? (
          <Button variant="ghost" className={styles.cancel} onClick={cancel} disabled={busy}>
            {busy ? "Cancelling…" : "Cancel"}
          </Button>
        ) : null}
      </div>
      {progress ? (
        <div className={styles.progressRow}>
          {share !== null ? (
            <>
              <span
                className={styles.meter}
                role="progressbar"
                aria-valuenow={share}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-label={`${job.kind} progress`}
              >
                <span className={styles.meterFill} style={{ width: `${share}%` }} />
              </span>
              <span className={styles.progressText}>
                {progress.current}/{progress.total}
                {progress.unit ? ` ${progress.unit}` : ""} · {share}%
              </span>
            </>
          ) : (
            <span className={styles.progressText}>
              {progress.message ?? `${progress.current}${progress.unit ? ` ${progress.unit}` : ""}`}
            </span>
          )}
        </div>
      ) : null}
      <div className={styles.whenRow}>
        <span>Started {formatWhen(job.startedAtMs ?? job.createdAtMs)}</span>
        {job.endedAtMs !== undefined ? <span>Ended {formatWhen(job.endedAtMs)}</span> : null}
      </div>
      {job.error ? (
        <div className={styles.jobError}>
          <ErrorNote
            error={{
              title: job.error.message,
              code: job.error.code,
              remediation: job.error.remediation ?? [],
              context: job.error.context,
            }}
          />
        </div>
      ) : null}
      {actionError ? (
        <div className={styles.jobError}>
          <ErrorNote error={actionError} />
        </div>
      ) : null}
    </li>
  );
}

export function JobsPage() {
  const jobs = useJobs((s) => s.jobs);
  const seedAll = useJobs((s) => s.seedAll);
  const [error, setError] = useState<DescribedError | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let live = true;
    setLoading(true);
    // The seed is the daemon's answer, not a guess: whatever the page
    // shows before it arrives is the empty truth of a fresh window.
    listJobs()
      .then((result) => {
        if (!live) return;
        seedAll(result.jobs);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (live) setError(describeError(cause));
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [seedAll]);

  const rows = Object.values(jobs).sort((a, b) => b.createdAtMs - a.createdAtMs);

  return (
    <div className={styles.page}>
      <header className={styles.pageHead}>
        <div>
          <h1 className={styles.title}>Jobs</h1>
          <p className={styles.subtitle}>
            Long-running operations — ZIM's service keeps the record; this page reads it.
          </p>
        </div>
      </header>

      {error ? (
        <div className={styles.alert} role="alert">
          <ErrorNote error={error} />
        </div>
      ) : null}

      {rows.length > 0 ? (
        <ul className={styles.list}>
          {rows.map((job) => (
            <JobRow key={job.jobId} job={job} />
          ))}
        </ul>
      ) : loading ? (
        <p className={styles.empty}>Reading the job record…</p>
      ) : error ? null : (
        // An empty page is only honest when the read actually answered —
        // a refusal claims nothing about the record (§82).
        <p className={styles.empty}>
          No jobs yet — backups, restores, publishes, and installs appear here while they run.
        </p>
      )}

      <p className={styles.note}>
        Finished jobs are kept by ZIM's service (the newest 50); the list re-reads when this page
        opens and stays live through job events while it is mounted.
      </p>
    </div>
  );
}

