// The honest answer to an unknown internal URL (ADR-0015, §58): a real
// page in the tab, with the history entry to leave it by. Never a silent
// redirect, never a fake search.

import styles from "./MissingPage.module.css";

export function MissingPage({ url }: { url: string }) {
  return (
    <div className={styles.page}>
      <div className={styles.card}>
        <h1 className={styles.title}>No such page</h1>
        <p className={styles.body}>
          <code className={styles.url}>{url}</code> is not a ZaminPanel page.
        </p>
        <p className={styles.body}>
          Internal pages live under <code className={styles.url}>zaminpanel://</code> — try{" "}
          <code className={styles.url}>zaminpanel://servers/</code> for the fleet.
        </p>
      </div>
    </div>
  );
}
