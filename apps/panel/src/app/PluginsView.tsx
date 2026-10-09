// Plugins: the operator installs from the Modrinth catalog the way the
// New Server flow installs software — search, pick, the daemon verifies
// and lands the jar (ADR-0012). The target directory is the daemon's
// honest answer (plugins/ or mods/, from the server's own layout); the
// installed list is the directory itself, never a shadow record.

import { useCallback, useEffect, useRef, useState } from "react";
import {
  pluginsDelete,
  pluginsInstall,
  pluginsInstalled,
  pluginsSearch,
  pluginsUpdates,
} from "../state/actions";
import type {
  PluginsInstalledResult,
  PluginsSearchResult,
  PluginsUpdatesResult,
} from "../protocol/types";
import { describeError } from "../state/errors";
import type { DescribedError } from "../state/errors";
import { ErrorNote } from "../ui/ErrorNote";
import { runningJob, useJobs } from "../state/jobs";
import { formatBytes } from "../state/metrics";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/PromptDialog";
import { IconPuzzle, IconSearch } from "../ui/icons";
import styles from "./PluginsView.module.css";

export function PluginsView({ serverId }: { serverId: string }) {
  const [search, setSearch] = useState("");
  const [result, setResult] = useState<PluginsSearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [installed, setInstalled] = useState<PluginsInstalledResult | null>(null);
  // The update check (ADR-0012's read side): the operator asks once, the
  // disk's bytes answer — chips ride the installed rows, never a second
  // list to keep in sync.
  const [updates, setUpdates] = useState<PluginsUpdatesResult | null>(null);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [error, setError] = useState<DescribedError | null>(null);
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
      .catch((cause: unknown) => setError(describeError(cause)));
  }, [serverId]);

  useEffect(() => {
    refreshInstalled();
    updatesLoaded.current = false;
    setUpdates(null);
  }, [refreshInstalled]);

  // Whether a report is on screen — a ref, so the quiet refresh after a
  // job can consult it without re-firing on every report change.
  const updatesLoaded = useRef(false);

  const checkUpdates = useCallback(() => {
    setCheckingUpdates(true);
    setError(null);
    void pluginsUpdates(serverId)
      .then((report) => {
        updatesLoaded.current = true;
        setUpdates(report);
      })
      .catch((cause: unknown) => setError(describeError(cause)))
      .finally(() => setCheckingUpdates(false));
  }, [serverId]);

  // Re-run the check quietly after a job lands — only when a report is
  // already on screen; the button stays the explicit entry point. A
  // failed quiet refresh keeps the old report; the next explicit check
  // replaces it.
  const refreshUpdatesIfLoaded = useCallback(() => {
    if (!updatesLoaded.current) return;
    void pluginsUpdates(serverId)
      .then(setUpdates)
      .catch(() => {});
  }, [serverId]);

  // When the install job completes (or before it starts), the busy flag
  // releases and the inventory refreshes for free — the jobs store is
  // the single source of "is something landing right now". A loaded
  // update report refreshes too: the verdicts are the disk's bytes, and
  // those just changed.
  useEffect(() => {
    if (installJob === undefined) {
      setInstallingProject(null);
      refreshInstalled();
      refreshUpdatesIfLoaded();
    }
  }, [installJob, refreshInstalled, refreshUpdatesIfLoaded]);

  const runSearch = useCallback(
    (query: string) => {
      setSearching(true);
      setError(null);
      void pluginsSearch(serverId, query)
        .then(setResult)
        .catch((cause: unknown) => setError(describeError(cause)))
        .finally(() => setSearching(false));
    },
    [serverId],
  );

  const install = useCallback(
    (projectId: string, replace = false, versionId?: string, retireFile?: string) => {
      setInstallingProject(projectId);
      setError(null);
      setPendingReplace(null);
      void pluginsInstall(serverId, projectId, versionId, replace, retireFile)
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
            setError(described);
          }
          setInstallingProject(null);
        });
    },
    [serverId],
  );

  // The question is the application's own ConfirmDialog — one slot holds
  // the file currently asked about.
  const [removing, setRemoving] = useState<string | null>(null);

  const remove = useCallback((fileName: string) => {
    setRemoving(fileName);
  }, []);

  const runRemove = useCallback(
    (fileName: string) => {
      setError(null);
      void pluginsDelete(serverId, fileName)
        .then(refreshInstalled)
        .catch((cause: unknown) => setError(describeError(cause)));
    },
    [serverId, refreshInstalled],
  );

  // The verdicts ride the installed rows — one map lookup, no second
  // list to keep in sync.
  const updatesByFile = new Map(
    (updates?.entries ?? []).map((entry) => [entry.fileName, entry]),
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
        <Button onClick={checkUpdates} disabled={checkingUpdates || busy}>
          {checkingUpdates ? "Checking…" : "Check updates"}
        </Button>
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
        <ErrorNote error={error} />
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
          Search the catalog to install a plugin — ZIM downloads and
          verifies it, straight into this server's plugin directory.
        </p>
      )}

      {installed && installed.entries.length > 0 ? (
        <div className={styles.installedBlock}>
          <span className={styles.installedLabel}>
            Installed — {installed.target}/
          </span>
          <ul className={styles.installedList}>
            {installed.entries.map((entry) => {
              const verdict = updatesByFile.get(entry.fileName);
              return (
                <li key={entry.fileName} className={styles.installed}>
                  <span className={styles.installedName}>{entry.fileName}</span>
                  <span className={styles.installedMeta}>
                    {formatBytes(entry.sizeBytes)}
                  </span>
                  {verdict ? (
                    <span
                      className={`${styles.updateChip} ${styles[verdict.status]}`}
                      title={
                        verdict.status === "update-available"
                          ? `${verdict.installedVersion ?? "installed"} → ${verdict.latestVersion ?? "newer"}`
                          : verdict.status === "unmanaged"
                            ? "These bytes are not the catalog's — the update flow cannot manage this jar."
                            : "The newest installable version is what this file already is."
                      }
                    >
                      {verdict.status === "update-available"
                        ? `update: ${verdict.latestVersion ?? "newer"}`
                        : verdict.status === "up-to-date"
                          ? "up to date"
                          : "unmanaged"}
                    </span>
                  ) : null}
                  {verdict?.status === "update-available" &&
                  verdict.projectId &&
                  verdict.latestVersionId ? (
                    <button
                      className={styles.remove}
                      disabled={busy}
                      onClick={() =>
                        install(
                          verdict.projectId ?? "",
                          true,
                          verdict.latestVersionId,
                          // The update retires the row it came from: the
                          // overwrite rule is name-keyed and a version
                          // bump usually changes the name, so without
                          // the retire the old jar would stay on disk —
                          // two versions of one plugin.
                          entry.fileName,
                        )
                      }
                    >
                      update
                    </button>
                  ) : entry.symlinkOutside ? (
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
              );
            })}
          </ul>
        </div>
      ) : null}
      {removing !== null ? (
        <ConfirmDialog
          title={`Remove ${removing} from the server?`}
          body="The plugin's jar is deleted from the server's plugins folder. This cannot be undone."
          confirmLabel="Remove"
          danger
          onConfirm={() => runRemove(removing)}
          onClose={() => setRemoving(null)}
        />
      ) : null}
    </section>
  );
}
