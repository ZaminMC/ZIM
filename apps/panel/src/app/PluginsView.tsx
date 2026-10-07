// Plugins: the operator installs from the Modrinth catalog the way the
// New Server flow installs software — search, pick, the daemon verifies
// and lands the jar (ADR-0012). The target directory is the daemon's
// honest answer (plugins/ or mods/, from the server's own layout); the
// installed list is the directory itself, never a shadow record.

import { useCallback, useEffect, useState } from "react";
import {
  pluginsDelete,
  pluginsInstall,
  pluginsInstalled,
  pluginsSearch,
} from "../state/actions";
import type { PluginsInstalledResult, PluginsSearchResult } from "../protocol/types";
import { describeError } from "../state/errors";
import { runningJob, useJobs } from "../state/jobs";
import { formatBytes } from "../state/metrics";
import { Button } from "../ui/Button";
import { IconPuzzle, IconSearch } from "../ui/icons";
import styles from "./PluginsView.module.css";

export function PluginsView({ serverId }: { serverId: string }) {
  const [search, setSearch] = useState("");
  const [result, setResult] = useState<PluginsSearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [installed, setInstalled] = useState<PluginsInstalledResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installingProject, setInstallingProject] = useState<string | null>(null);
  // The typed update rule (ADR-0012): PLUGIN_EXISTS offers the explicit
  // overwrite — the operator's decision, surfaced once, never silently.
  const [pendingReplace, setPendingReplace] = useState<{
    projectId: string;
    file: string;
  } | null>(null);

  // The live install job (if any) — progress chips come from the
  // reconciled jobs store, the same as backups and server creation.
  const installJob = useJobs((s) => runningJob(s.jobs, serverId, "plugins.install"));
  const busy = installJob !== undefined || installingProject !== null;

  const refreshInstalled = useCallback(() => {
    void pluginsInstalled(serverId)
      .then(setInstalled)
      .catch((cause: unknown) => setError(describeError(cause).title));
  }, [serverId]);

  useEffect(() => {
    refreshInstalled();
  }, [refreshInstalled]);

  // When the install job completes (or before it starts), the busy flag
  // releases and the inventory refreshes for free — the jobs store is
  // the single source of "is something landing right now".
  useEffect(() => {
    if (installJob === undefined) {
      setInstallingProject(null);
      refreshInstalled();
    }
  }, [installJob, refreshInstalled]);

  const runSearch = useCallback(
    (query: string) => {
      setSearching(true);
      setError(null);
      void pluginsSearch(serverId, query)
        .then(setResult)
        .catch((cause: unknown) => setError(describeError(cause).title))
        .finally(() => setSearching(false));
    },
    [serverId],
  );

  const install = useCallback(
    (projectId: string, replace = false) => {
      setInstallingProject(projectId);
      setError(null);
      setPendingReplace(null);
      void pluginsInstall(serverId, projectId, undefined, replace)
        .then(() => {
          // The job store owns the rest; the progress chip shows itself.
        })
        .catch((cause: unknown) => {
          const described = describeError(cause);
          if (described.code === "PLUGIN_EXISTS") {
            const file = described.context?.file;
            setPendingReplace({
              projectId,
              file: typeof file === "string" ? file : "a file by this name",
            });
          } else {
            setError(described.title);
          }
          setInstallingProject(null);
        });
    },
    [serverId],
  );

  const remove = useCallback(
    (fileName: string) => {
      if (!window.confirm(`Remove ${fileName} from the server?`)) return;
      setError(null);
      void pluginsDelete(serverId, fileName)
        .then(refreshInstalled)
        .catch((cause: unknown) => setError(describeError(cause).title));
    },
    [serverId, refreshInstalled],
  );

  return (
    <section className={styles.plugins} aria-label="Plugins">
      <div className={styles.head}>
        <IconPuzzle size={15} />
        <span className={styles.target}>
          {installed
            ? `Installs land in ${installed.target}/`
            : "The plugin catalog (Modrinth) — installs land where this server keeps them"}
        </span>
        <span className={styles.spacer} />
        <form
          className={styles.searchForm}
          onSubmit={(event) => {
            event.preventDefault();
            runSearch(search);
          }}
        >
          <input
            className={styles.search}
            type="search"
            placeholder="Search plugins…"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            aria-label="Search the plugin catalog"
          />
          <Button type="submit" disabled={searching}>
            <IconSearch size={13} />
            {searching ? "Searching…" : "Search"}
          </Button>
        </form>
        <Button onClick={refreshInstalled}>Refresh</Button>
      </div>

      {installJob ? (
        <div className={styles.job} role="status">
          <span className={styles.jobBar} aria-hidden="true">
            <span
              className={styles.jobFill}
              style={{
                width:
                  installJob.progress?.total && installJob.progress.total > 0
                    ? `${Math.min((installJob.progress.current / installJob.progress.total) * 100, 100)}%`
                    : "20%",
              }}
            />
          </span>
          <span className={styles.jobText}>
            {installJob.progress?.message ?? "installing…"}
          </span>
        </div>
      ) : null}

      {pendingReplace ? (
        <div className={styles.alert} role="alert">
          <span>
            {pendingReplace.file} is already installed with different
            content. Replace it with the published file?
          </span>
          <Button
            variant="primary"
            disabled={busy}
            onClick={() => install(pendingReplace.projectId, true)}
          >
            Replace
          </Button>
          <Button onClick={() => setPendingReplace(null)}>Dismiss</Button>
        </div>
      ) : null}

      {error ? (
        <div className={styles.alert} role="alert">
          {error}
        </div>
      ) : null}

      {result ? (
        result.hits.length === 0 ? (
          <p className={styles.emptyNote}>Nothing found for that search.</p>
        ) : (
          <ul className={styles.results}>
            {result.hits.map((hit) => (
              <li key={hit.projectId} className={styles.hit}>
                <div className={styles.hitBody}>
                  <span className={styles.hitTitle}>{hit.title}</span>
                  <span className={styles.hitDesc}>{hit.description}</span>
                  <span className={styles.hitMeta}>
                    {hit.downloads.toLocaleString()} downloads
                    {hit.loaders.length > 0 ? ` · ${hit.loaders.join(", ")}` : ""}
                  </span>
                </div>
                <Button
                  variant="primary"
                  disabled={busy}
                  busy={installingProject === hit.projectId}
                  onClick={() => install(hit.projectId)}
                >
                  Install
                </Button>
              </li>
            ))}
          </ul>
        )
      ) : (
        <p className={styles.emptyNote}>
          Search the catalog to install a plugin — the daemon downloads and
          verifies it, straight into this server's plugin directory.
        </p>
      )}

      {installed && installed.entries.length > 0 ? (
        <div className={styles.installedBlock}>
          <span className={styles.installedLabel}>
            Installed — {installed.target}/
          </span>
          <ul className={styles.installedList}>
            {installed.entries.map((entry) => (
              <li key={entry.fileName} className={styles.installed}>
                <span className={styles.installedName}>{entry.fileName}</span>
                <span className={styles.installedMeta}>
                  {formatBytes(entry.sizeBytes)}
                </span>
                {entry.symlinkOutside ? (
                  <span className={styles.badge}>outside root</span>
                ) : (
                  <button
                    className={styles.remove}
                    disabled={busy}
                    onClick={() => remove(entry.fileName)}
                  >
                    remove
                  </button>
                )}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </section>
  );
}
