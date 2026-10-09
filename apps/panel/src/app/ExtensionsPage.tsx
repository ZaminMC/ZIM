// zim://extensions/ — the extensions room (§56/§57, ADR-0031).
// The daemon's inventory: who is installed, which permissions each
// extension claims from the closed, deny-by-default vocabulary, and
// which folders could not be answered for (named, never skipped). The
// execution/contribution model is reserved by the same ADR, and the
// page states it in its own render — an inventory is not a runtime.

import { useEffect, useState } from "react";
import { listExtensions } from "../state/actions";
import { describeError, type DescribedError } from "../state/errors";
import type { ExtensionProblem, ExtensionView, ExtensionsListResult } from "../protocol/types";
import { ErrorNote } from "../ui/ErrorNote";
import shared from "./internalPage.module.css";
import styles from "./ExtensionsPage.module.css";

function ExtensionRow({ extension }: { extension: ExtensionView }) {
  return (
    <li className={shared.row}>
      <div className={shared.rowHead}>
        <span className={styles.name}>{extension.name}</span>
        <span className={styles.version}>v{extension.version}</span>
        <span className={styles.id}>{extension.id}</span>
      </div>
      {extension.description ? (
        <p className={styles.description}>{extension.description}</p>
      ) : null}
      {extension.permissions.length === 0 ? (
        <p className={styles.noPerms}>Declares no permissions — nothing will be granted.</p>
      ) : (
        <ul className={styles.perms} aria-label={`Permissions declared by ${extension.name}`}>
          {extension.permissions.map((permission) => (
            <li
              key={permission}
              className={styles.perm}
              data-family={permission.startsWith("data:") ? "data" : "contribution"}
            >
              {permission}
            </li>
          ))}
        </ul>
      )}
      <p className={styles.dir}>Folder: {extension.directory}</p>
    </li>
  );
}

function ProblemRow({ problem }: { problem: ExtensionProblem }) {
  return (
    <li className={styles.problem}>
      <span className={styles.problemDir}>{problem.directory}</span>
      <span className={styles.problemReason}>{problem.reason}</span>
    </li>
  );
}

export function ExtensionsPage() {
  const [result, setResult] = useState<ExtensionsListResult | null>(null);
  const [error, setError] = useState<DescribedError | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    listExtensions()
      .then((answer) => {
        setResult(answer);
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause)))
      .finally(() => setLoading(false));
  }, []);

  if (error) {
    return (
      <div className={shared.page}>
        <header className={shared.head}>
          <h1 className={shared.title}>Extensions</h1>
        </header>
        <ErrorNote error={error} />
      </div>
    );
  }

  return (
    <div className={shared.page}>
      <header className={shared.head}>
        <h1 className={shared.title}>Extensions</h1>
        <p className={shared.subtitle}>
          Typed addons with declared permissions. Deny-by-default: a permission an extension does
          not claim is never granted.
        </p>
      </header>

      {loading ? <p className={shared.note}>Reading the inventory…</p> : null}

      {result !== null && !loading ? (
        <>
          {result.extensions.length === 0 && result.problems.length === 0 ? (
            <p className={shared.note}>
              No extensions installed. Extension folders live in{" "}
              <code className={shared.mono}>{result.directory}</code>; each one carries a{" "}
              <code className={shared.mono}>zamin-extension.toml</code> manifest.
            </p>
          ) : null}

          <ul className={shared.list} aria-label="Installed extensions">
            {result.extensions.map((extension) => (
              <ExtensionRow key={extension.id} extension={extension} />
            ))}
          </ul>

          {result.problems.length > 0 ? (
            <>
              <h2 className={styles.sectionTitle}>Folders that could not be read</h2>
              <ul className={styles.problems} aria-label="Unreadable extension folders">
                {result.problems.map((problem) => (
                  <ProblemRow key={problem.directory} problem={problem} />
                ))}
              </ul>
            </>
          ) : null}

          {!result.contributionsActive ? (
            <p className={styles.reservedNote} role="note">
              Extensions declare permissions; nothing contributes yet. Context-menu entries,
              sidebar pages, and tools are the reserved room (ADR-0031) — this inventory is the
              declaration half of the model, and it is the part that makes the rest auditable.
            </p>
          ) : null}
        </>
      ) : null}
    </div>
  );
}
