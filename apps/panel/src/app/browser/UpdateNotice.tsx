// The update notice (ADR-0024): the chrome's one update row, rendered only
// when the lane has something to say — a running install, a pending
// restart, or a failure (§81: the message is a human sentence carrying the
// technical detail). Idle, "checking" and "up to date" are chrome-silent:
// an update row that nagged about good news would train the operator to
// ignore it. "Available" renders in the Settings updates rows, where the
// decision (auto-install off) belongs. The copy is the store's single
// source — updatesSentence — so the banner and the settings rows can
// never disagree.

import { updatesSentence, useUpdates } from "../../state/updates";
import { Button } from "../../ui/Button";
import styles from "./UpdateNotice.module.css";

export function UpdateNotice() {
  const phase = useUpdates((s) => s.phase);
  const restart = useUpdates((s) => s.restart);
  const dismiss = useUpdates((s) => s.dismiss);
  const check = useUpdates((s) => s.check);

  switch (phase.kind) {
    case "idle":
    case "unavailable":
    case "checking":
    case "upToDate":
    case "available":
      return null;
    case "downloading":
      return (
        <div className={styles.row} data-tone="active" role="status">
          <span className={styles.message}>{updatesSentence(phase)}</span>
        </div>
      );
    case "ready":
      return (
        <div className={styles.row} data-tone="active" role="status">
          <span className={styles.message}>{updatesSentence(phase)}</span>
          <div className={styles.actions}>
            <Button variant="primary" onClick={() => void restart()}>
              Restart now
            </Button>
          </div>
        </div>
      );
    case "error":
      return (
        <div className={styles.row} data-tone="error" role="alert">
          <span className={styles.message} title={phase.message}>
            {updatesSentence(phase)}
          </span>
          <div className={styles.actions}>
            <Button variant="ghost" onClick={() => void check(true)}>
              Retry
            </Button>
            <Button variant="ghost" onClick={dismiss}>
              Dismiss
            </Button>
          </div>
        </div>
      );
  }
}
