// The crash card (§62): the server tab stays open, the status turns
// red, and the operator gets the founder's exact conversation —
// "Server stopped unexpectedly." with the Reason the daemon classified,
// then [Restart], [View logs], [Ask Dutchmen]. The card never invents a
// cause: what it says is the evidence excerpt and the structured error
// the daemon attached at crash time, and nothing more.
//
// Restart dispatches through the SAME pending/actionError fields the
// header uses, so one in-flight verb shows once and one failure
// surfaces once. The card resolves itself when the next state
// transition proves it stale (the store's rule); the buttons only
// dispatch and wait. "Ask Dutchmen" is the reserved room — the agent
// runtime will own crash inspection, and the seat is honestly labeled
// as not arrived rather than silently absent.

import { useState } from "react";
import { startServer } from "../state/actions";
import { describeError } from "../state/errors";
import { useServers, type CrashInfo } from "../state/servers";
import { useUi } from "../state/ui";
import { Button } from "../ui/Button";
import styles from "./CrashCard.module.css";

/** The honest reason line: the daemon's structured message if it has
 *  one, else the first evidence line. Null when nothing was captured —
 *  the card then says so instead of guessing. */
export function crashReason(crash: CrashInfo): string | null {
  if (crash.error?.message) return crash.error.message;
  const firstLine = crash.evidence?.split("\n").find((line) => line.trim() !== "");
  return firstLine ?? null;
}

export function CrashCard({
  serverId,
  onViewLogs,
  onRecover,
}: {
  serverId: string;
  /** Jumps to the log viewer (§62's [View logs]). */
  onViewLogs?: () => void;
  /** Opens the backups surface (restore flow). */
  onRecover?: () => void;
}) {
  const crash = useServers((s) => s.crashes[serverId]);
  const resolveCrash = useServers((s) => s.resolveCrash);
  const setPending = useUi((s) => s.setPending);
  const setActionError = useUi((s) => s.setActionError);
  const [restarting, setRestarting] = useState(false);
  if (!crash || crash.resolved) return null;

  const reason = crashReason(crash);

  const restart = () => {
    setActionError(serverId, null);
    setPending(serverId, "start");
    setRestarting(true);
    void startServer(serverId)
      .catch((error: unknown) => {
        const described = describeError(error);
        setActionError(serverId, {
          code: described.code,
          message: described.title,
          remediation: described.remediation,
        });
      })
      .finally(() => {
        setPending(serverId, null);
        setRestarting(false);
      });
  };

  return (
    <aside className={styles.card} role="status" aria-label="Crash report">
      <div className={styles.head}>
        <h2 className={styles.title}>Server stopped unexpectedly.</h2>
        <button
          className={styles.dismiss}
          onClick={() => resolveCrash(serverId)}
          aria-label="Dismiss crash report"
        >
          ×
        </button>
      </div>

      <div className={styles.facts}>
        <span className={styles.chip}>during {crash.phase}</span>
        <span className={styles.chip}>exit code: {crash.exitCode ?? "unknown"}</span>
        {crash.error?.code ? <span className={styles.chip}>{crash.error.code}</span> : null}
      </div>

      {reason ? (
        <p className={styles.reason}>
          <strong>Reason:</strong> {reason}
        </p>
      ) : (
        <p className={styles.reason}>
          <strong>Reason:</strong> nothing was captured — the logs are the evidence now.
        </p>
      )}

      {crash.evidence && crash.evidence.trim() !== reason ? (
        <pre className={styles.evidence}>{crash.evidence}</pre>
      ) : null}

      {crash.error?.remediation && crash.error.remediation.length > 0 ? (
        <ul className={styles.remediation}>
          {crash.error.remediation.map((step) => (
            <li key={step}>{step}</li>
          ))}
        </ul>
      ) : null}

      <div className={styles.actions}>
        <Button variant="primary" onClick={restart} disabled={restarting}>
          {restarting ? "Starting…" : "Restart"}
        </Button>
        {onViewLogs ? <Button onClick={onViewLogs}>View logs</Button> : null}
        <Button
          onClick={() => {}}
          disabled
          title="Reserved — Dutchmen inspects crashes once the agent runtime arrives."
        >
          Ask Dutchmen
        </Button>
        {onRecover ? <Button onClick={onRecover}>Recover from a backup</Button> : null}
      </div>
    </aside>
  );
}
