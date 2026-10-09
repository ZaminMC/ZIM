// The error note (§81): one shared body for every error alert in the
// panel. The founder's bar is three-part — the failure is a human
// sentence; the "what to do next" rides it when the protocol typed one;
// and the technical details (code, structured context) stay available
// behind [View details] instead of either vanishing or shouting. The
// owning view keeps its alert frame (class, role, verbs); this renders
// only the content, so no surface re-decides the shape and every surface
// grows the disclosure at once.

import type { DescribedError } from "../state/errors";
import styles from "./ErrorNote.module.css";

export function ErrorNote({ error }: { error: DescribedError }) {
  const hasContext =
    error.context !== undefined && Object.keys(error.context).length > 0;
  const hasDetails = error.code !== undefined || hasContext;
  return (
    <>
      <div className={styles.title}>{error.title}</div>
      {error.remediation.length > 0 ? (
        <ul className={styles.remediation}>
          {error.remediation.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      ) : null}
      {hasDetails ? (
        <details className={styles.details}>
          <summary className={styles.summary}>View details</summary>
          <pre className={styles.code}>
            {[
              error.code !== undefined ? `code: ${error.code}` : null,
              hasContext ? `context:\n${JSON.stringify(error.context, null, 2)}` : null,
            ]
              .filter((part): part is string => part !== null)
              .join("\n\n")}
          </pre>
        </details>
      ) : null}
    </>
  );
}
