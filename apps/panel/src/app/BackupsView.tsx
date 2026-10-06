// Backups: the server's archive list and the two job-backed operations —
// create (with the daemon's live save window when the server is running)
// and restore (refused by the daemon while the server runs, so the button
// is disabled up front with the reason, not after a round trip). Progress
// arrives as job events (ADR-0006), not as reply payloads.

import { useCallback, useEffect, useState } from "react";
import { createBackup, listBackups, restoreBackup } from "../state/actions";
import { describeError } from "../state/errors";
import { useJobs } from "../state/jobs";
import type { BackupInfo } from "../protocol/types";
import { Button } from "../ui/Button";
import styles from "./BackupsView.module.css";

function formatWhen(ms: number): string {
  return new Date(ms).toLocaleString();
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${bytes} B`;
}

export function BackupsView({ serverId, running }: { serverId: string; running: boolean }) {
  const [backups, setBackups] = useState<BackupInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState<string | null>(null);

  const jobs = useJobs((s) => s.jobs);
  const createRunning = Object.values(jobs).find(
    (job) => job.serverId === serverId && job.kind === "backup.create" && job.state === "running",
  );
  const restoreRunning = Object.values(jobs).find(
    (job) => job.serverId === serverId && job.kind === "backup.restore" && job.state === "running",
  );

  const refresh = useCallback(() => {
    void listBackups(serverId)
      .then((result) => {
        setBackups(result.backups);
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause).title));
  }, [serverId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // A finished job changes what exists on disk — re-list when the runner
  // reports completion for this server.
  const lastFinished = Object.values(jobs)
    .filter(
      (job) =>
        job.serverId === serverId &&
        job.state !== "running" &&
        job.state !== "queued" &&
        job.endedAtMs !== undefined,
    )
    .sort((a, b) => (b.endedAtMs ?? 0) - (a.endedAtMs ?? 0))[0];
  useEffect(() => {
    if (lastFinished === undefined) return;
    refresh();
    if (lastFinished.state === "succeeded") {
      setNotice(
        lastFinished.kind === "backup.create"
          ? "Backup created."
          : "Restore finished — the server files were replaced.",
      );
    } else if (lastFinished.state === "failed") {
      setNotice(`The last ${lastFinished.kind === "backup.create" ? "backup" : "restore"} failed.`);
    } else if (lastFinished.state === "cancelled") {
      setNotice("The last job was cancelled.");
    }
  }, [lastFinished, refresh]);

  const runCreate = () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    void createBackup(serverId)
      .then(() => setNotice("Backup started — progress shows below."))
      .catch((cause: unknown) => setError(describeError(cause).title))
      .finally(() => setBusy(false));
  };

  const runRestore = (backupId: string) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    setConfirming(null);
    void restoreBackup(serverId, backupId)
      .then(() => setNotice("Restore started — the server must stay stopped."))
      .catch((cause: unknown) => setError(describeError(cause).title))
      .finally(() => setBusy(false));
  };

  const jobLine = createRunning ?? restoreRunning;
  const progress = jobLine?.progress;

  return (
    <section className={styles.backups} aria-label="Backups">
      <div className={styles.head}>
        <span className={styles.count}>{backups ? `${backups.length} backups` : "loading…"}</span>
        <span className={styles.spacer} />
        <Button onClick={runCreate} disabled={busy || createRunning !== undefined || restoreRunning !== undefined}>
          {createRunning !== undefined ? "Backing up…" : "Create backup"}
        </Button>
      </div>

      <p className={styles.note}>
        {running
          ? "Creating now uses the live save window (save-off → save-all → archive → save-on). Restores need the server stopped."
          : "The server is stopped: backups are cold copies, and restores are allowed."}
      </p>

      {jobLine && progress ? (
        <div className={styles.job} role="status">
          <span className={styles.jobKind}>{jobLine.kind === "backup.create" ? "Backup" : "Restore"}</span>
          {progress.total !== undefined && progress.total > 0 ? (
            <>
              <progress
                className={styles.bar}
                max={progress.total}
                value={Math.min(progress.current, progress.total)}
              />
              <span className={styles.jobMeta}>
                {Math.min(100, Math.round((progress.current / progress.total) * 100))}%
              </span>
            </>
          ) : (
            <span className={styles.jobMeta}>{progress.current.toLocaleString()} {progress.unit ?? ""}</span>
          )}
          {progress.message ? <span className={styles.jobMessage}>{progress.message}</span> : null}
        </div>
      ) : null}

      {error ? (
        <div className={styles.alert} role="alert">
          {error}
        </div>
      ) : null}
      {notice && !error ? (
        <div className={styles.notice} role="status">
          {notice}
        </div>
      ) : null}

      {backups && backups.length === 0 ? (
        <p className={styles.empty}>
          No backups yet. The first one snapshots the whole server root (world, configs, plugins —
          everything except staging areas).
        </p>
      ) : null}

      {backups && backups.length > 0 ? (
        <ul className={styles.list}>
          {backups.map((backup) => (
            <li key={backup.backupId} className={styles.item}>
              <div className={styles.itemMain}>
                <span className={styles.when}>{formatWhen(backup.createdAtMs)}</span>
                <span className={styles.meta}>
                  {formatBytes(backup.sizeBytes)} · {backup.fileCount} files · {backup.taken}
                </span>
                {backup.label ? <span className={styles.label}>{backup.label}</span> : null}
              </div>
              {confirming === backup.backupId ? (
                <div className={styles.confirm}>
                  <span>Replace the server files with this backup?</span>
                  <Button variant="danger" disabled={busy || running} onClick={() => runRestore(backup.backupId)}>
                    Yes, restore
                  </Button>
                  <Button onClick={() => setConfirming(null)}>Keep current files</Button>
                </div>
              ) : (
                <Button
                  variant="danger"
                  disabled={running || busy || restoreRunning !== undefined}
                  title={
                    running
                      ? "Stop the server before restoring — open files cannot be replaced"
                      : undefined
                  }
                  onClick={() => setConfirming(backup.backupId)}
                >
                  Restore
                </Button>
              )}
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}
