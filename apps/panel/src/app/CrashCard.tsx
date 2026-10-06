// Crash card: phase, exit code, and the evidence excerpt the daemon
// classified at crash time (ADR-0005). Stays until the server starts
// again (resolved by the next transition) or the operator dismisses it.

import { useServers } from "../state/servers";
import { Button } from "../ui/Button";
import styles from "./CrashCard.module.css";

export function CrashCard({ serverId }: { serverId: string }) {
  const crash = useServers((s) => s.crashes[serverId]);
  const resolveCrash = useServers((s) => s.resolveCrash);
  if (!crash) return null;

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
      </div>
    </aside>
  );
}
