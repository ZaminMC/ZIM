// The new tab's machine section (§64, ADR-0027): what the daemon's
// discovery scan holds beyond the registry — server directories and
// supported jars, each with its evidence and, for a directory, the one
// verb that turns it into a managed server (register, then open). The
// section is a live answer: it follows the query with a debounce, states
// its loading, names its budget truth, and never pretends a jar is a
// server.

import { useEffect, useRef, useState } from "react";
import { discoverServers } from "../../state/actions";
import type { DiscoveredServer, DiscoverResult } from "../../protocol/types";
import { ErrorNote } from "../../ui/ErrorNote";
import { describeError } from "../../state/errors";
import styles from "./NewTabPage.module.css";

/** The register-and-open verb's id rules mirror the daemon's ServerId:
 *  lowercase alphanumerics with dashes/underscores. A folder named
 *  "My Server 2!" becomes "my-server-2". */
export function slugFromPath(path: string): string {
  const base = path.split(/[\\/]/).filter(Boolean).pop() ?? "server";
  const slug = base
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
  return slug === "" || /^[^a-z0-9]/.test(slug) ? `s-${slug}` : slug;
}

const DEBOUNCE_MS = 400;

export function useMachineDiscovery(query: string) {
  const [phase, setPhase] = useState<
    { kind: "idle" } | { kind: "loading" } | { kind: "error"; note: string }
  >({ kind: "idle" });
  const [rows, setRows] = useState<DiscoveredServer[]>([]);
  const [skipped, setSkipped] = useState<string[]>([]);
  const [truncated, setTruncated] = useState(false);
  const answered = useRef("");

  useEffect(() => {
    const needle = query.trim();
    const timer = window.setTimeout(() => {
      answered.current = needle;
      setPhase({ kind: "loading" });
      discoverServers(needle === "" ? undefined : needle)
        .then((result: DiscoverResult) => {
          if (answered.current !== needle) return;
          setRows(result.servers);
          setSkipped(result.skippedRoots);
          setTruncated(result.truncated);
          setPhase({ kind: "idle" });
        })
        .catch((error: unknown) => {
          if (answered.current !== needle) return;
          const described = describeError(error);
          setPhase({ kind: "error", note: described.title });
        });
    }, needle === "" ? 0 : DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [query]);

  return { phase, rows, skipped, truncated };
}

export function MachineRows({
  rows,
  onOpen,
}: {
  rows: DiscoveredServer[];
  onOpen: (candidate: DiscoveredServer) => void;
}) {
  const candidates = rows.filter((row) => row.kind !== "registered");
  if (candidates.length === 0) return null;
  return (
    <ul className={styles.list}>
      {candidates.map((row) => (
        <li key={row.path}>
          <button
            className={styles.row}
            onClick={() => onOpen(row)}
            aria-label={
              row.kind === "directory"
                ? `Open ${row.displayName ?? row.path}`
                : `Do something with ${row.jarName ?? row.path}`
            }
          >
            <span className={styles.kindChip} data-kind={row.kind}>
              {row.kind === "directory" ? "folder" : "jar"}
            </span>
            <span className={styles.name}>{row.displayName ?? row.jarName ?? row.path}</span>
            <span className={styles.addr}>
              {[
                row.port ? `:${row.port}` : null,
                row.platform ?? null,
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

export function MachineDiscovery({
  query,
  onOpen,
}: {
  query: string;
  onOpen: (candidate: DiscoveredServer) => void;
}) {
  const { phase, rows, skipped, truncated } = useMachineDiscovery(query);
  if (phase.kind === "error") {
    return (
      <div className={styles.machine} role="alert">
        <ErrorNote
          error={{
            title: "The scan could not run.",
            remediation: [phase.note],
          }}
        />
      </div>
    );
  }
  const hasContent = phase.kind === "loading" || rows.length > 0;
  if (!hasContent && skipped.length === 0) return null;
  return (
    <section className={styles.machine} aria-label="On this machine">
      <h2 className={styles.machineTitle}>On this machine</h2>
      {phase.kind === "loading" ? (
        <p className={styles.machineNote} role="status">
          Scanning the configured roots…
        </p>
      ) : null}
      <MachineRows rows={rows} onOpen={onOpen} />
      {truncated ? (
        <p className={styles.machineNote}>
          The scan hit its budget — some entries may be missing. Narrow the search or trim the
          roots.
        </p>
      ) : null}
      {skipped.length > 0 ? (
        <p className={styles.machineNote}>
          {skipped.length === 1
            ? "A configured root could not be read:"
            : "Some configured roots could not be read:"}{" "}
          {skipped.join(", ")}
        </p>
      ) : null}
    </section>
  );
}
