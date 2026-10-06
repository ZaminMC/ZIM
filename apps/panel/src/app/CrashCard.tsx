// Crash card: phase, exit code, and the evidence excerpt the daemon
// classified at crash time (ADR-0005). Stays until the server starts
// again (resolved by the next transition) or the operator dismisses it —
// both paths flip `resolved` in the store, which hides the card.
//
// Crash recovery (Phase 5): the card is also the doorway to the backups
// surface — restore-from-backup is the recovery path when a crash left
// the server files unusable, so the card offers the jump directly.

import { useServers } from "../state/servers";
import { Button } from "../ui/Button";
import styles from "./CrashCard.module.css";

export function CrashCard({
  serverId,
  onRecover,
}: {
  serverId: string;
  /** Opens the backups surface (restore flow). */
  onRecover?: () => void;
}) {
  const crash = useServers((s) => s.crashes[serverId]);
  const resolveCrash = useServers((s) => s.resolveCrash);
  if (!crash || crash.resolved) return null;

  return (
    <aside className={styles.card} role="status" aria-label="Crash report">
      <div className={styles.head}>
        <h2 className={styles.title}>The server crashed during {crash.phase}</h2>
        <button
          className={styles.dismiss}
          onClick={() => resolveCrash(serverId)}
          aria-label="Dismiss crash report"
        >
          ×
        </button>
      </div>

      <div className={styles.facts}>
        <span className={styles.chip}>exit code: {crash.exitCode ?? "unknown"}</span>
        {crash.error?.code ? <span className={styles.chip}>{crash.error.code}</span> : null}
      </div>

      {crash.error?.message ? <p style={{ margin: 0 }}>{crash.error.message}</p> : null}

      {crash.evidence ? (
        <pre className={styles.evidence}>{crash.evidence}</pre>
      ) : (
        <p style={{ margin: 0, color: "var(--text-muted)" }}>
          No evidence excerpt was captured for this crash.
        </p>
      )}

      {crash.error?.remediation && crash.error.remediation.length > 0 ? (
        <ul className={styles.remediation}>
          {crash.error.remediation.map((step) => (
            <li key={step}>{step}</li>
          ))}
        </ul>
      ) : null}

      <div className={styles.actions}>
        <Button variant="primary" onClick={() => resolveCrash(serverId)}>
          Acknowledge
        </Button>
        {onRecover ? (
          <Button onClick={onRecover}>Recover from a backup</Button>
        ) : null}
      </div>
    </aside>
  );
}
