// New Server flow, two doors. The default door is the Phase 6 path:
// pick software from the catalog, pick a version and build, and the
// daemon downloads and registers everything — zero manual JAR handling.
// The other door registers an existing directory (the honest Phase 1–2
// path, kept for servers that already exist on disk). Everything after
// this references the id only (ADR-0004).

import { useEffect, useState } from "react";
import {
  catalogBuilds,
  catalogList,
  catalogVersions,
  createServer,
  getServer,
  installJava,
  listJava,
  registerServer,
} from "../state/actions";
import { describeError } from "../state/errors";
import type { DescribedError } from "../state/errors";
import { useJobs } from "../state/jobs";
import { useServers } from "../state/servers";
import { useTabs } from "../state/tabs";
import { useUi } from "../state/ui";
import type { CatalogBuild, CatalogEntry, JavaRuntime } from "../protocol/types";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import styles from "./NewServerModal.module.css";

/** Client-side sanity only; the daemon owns the authoritative id rules. */
export function validateServerId(value: string): string | null {
  if (value.length === 0) return "A server id is required.";
  if (!/^[a-z0-9][a-z0-9-]{0,63}$/.test(value)) {
    return "Use lowercase letters, digits, and dashes (max 64 chars).";
  }
  return null;
}

export function formatProgressBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${n} B`;
}

/** The runtime that would serve a required major, or null. */
export function satisfyingRuntime(
  runtimes: JavaRuntime[],
  required: number | null,
): JavaRuntime | undefined {
  if (required === null) return undefined;
  return runtimes.find((r) => r.major >= required);
}

export function NewServerModal() {
  const close = useUi((s) => s.setNewServerOpen);
  const navigate = useTabs((s) => s.navigate);
  const upsert = useServers((s) => s.upsert);

  const [mode, setMode] = useState<"download" | "register">("download");

  // Register-existing mode state (the honest Phase 1–2 form).
  const [rootPath, setRootPath] = useState("");

  // Shared fields.
  const [serverId, setServerId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [idError, setIdError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<DescribedError | null>(null);

  // Download mode: catalog pickers.
  const [entries, setEntries] = useState<CatalogEntry[] | null>(null);
  const [project, setProject] = useState<string>("");
  const [versions, setVersions] = useState<{ id: string }[] | null>(null);
  const [version, setVersion] = useState<string>("");
  const [builds, setBuilds] = useState<CatalogBuild[] | null>(null);
  const [buildId, setBuildId] = useState<number | null>(null);
  // The Fabric family's stand-in for builds: stable loader versions.
  const [loaders, setLoaders] = useState<string[] | null>(null);
  const [loaderId, setLoaderId] = useState<string>("");
  const [javaMajor, setJavaMajor] = useState<number | null>(null);
  // The Join page's "create server on this port" prefill rides the ui
  // store: consumed once on open, then cleared (a later open starts
  // clean).
  const prefillPort = useUi((s) => s.newServerPort);
  const setNewServerPortPrefill = useUi((s) => s.setNewServerPort);
  const [portText, setPortText] = useState(prefillPort ?? "");
  useEffect(() => {
    if (prefillPort != null) {
      setPortText(prefillPort);
      setNewServerPortPrefill(null);
    }
  }, [prefillPort, setNewServerPortPrefill]);

  // Download mode: java runtimes.
  const [runtimes, setRuntimes] = useState<JavaRuntime[] | null>(null);
  const [javaPath, setJavaPath] = useState<string>("auto");
  const [installJobId, setInstallJobId] = useState<string | null>(null);

  // Download mode: the create job the modal watches to completion.
  const [createJobId, setCreateJobId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const installJob = useJobs((s) => (installJobId ? s.jobs[installJobId] : undefined));
  const createJob = useJobs((s) => (createJobId ? s.jobs[createJobId] : undefined));

  // The catalog is small and static per session: load once.
  useEffect(() => {
    if (mode !== "download") return;
    let alive = true;
    catalogList()
      .then((result) => {
        if (!alive) return;
        setEntries(result.entries);
        const first = result.entries[0];
        if (first !== undefined) {
          setProject((current) => current || first.id);
        }
      })
      .catch((error: unknown) => {
        if (alive) setSubmitError(describeError(error));
      });
    return () => {
      alive = false;
    };
  }, [mode]);

  // Versions follow the project.
  useEffect(() => {
    if (mode !== "download" || !project) return;
    let alive = true;
    setVersions(null);
    setVersion("");
    catalogVersions(project)
      .then((result) => {
        if (!alive) return;
        setVersions(result.versions);
        const newest = result.versions[0];
        if (newest !== undefined) setVersion(newest.id);
      })
      .catch((error: unknown) => {
        if (alive) setSubmitError(describeError(error));
      });
    return () => {
      alive = false;
    };
  }, [mode, project]);

  // Builds (or the Fabric family's loader list) follow the version.
  useEffect(() => {
    if (mode !== "download" || !project || !version) return;
    let alive = true;
    setBuilds(null);
    setBuildId(null);
    setLoaders(null);
    setLoaderId("");
    setJavaMajor(null);
    catalogBuilds(project, version)
      .then((result) => {
        if (!alive) return;
        setBuilds(result.builds);
        setJavaMajor(result.javaMajor ?? null);
        setLoaders(result.loaders ?? null);
        const newestBuild = result.builds[0];
        if (newestBuild !== undefined) setBuildId(newestBuild.id);
        const newestLoader = result.loaders?.[0];
        if (newestLoader !== undefined) setLoaderId(newestLoader);
      })
      .catch((error: unknown) => {
        if (alive) setSubmitError(describeError(error));
      });
    return () => {
      alive = false;
    };
  }, [mode, project, version]);

  // The runtime list refreshes on mount and after an install completes.
  useEffect(() => {
    if (mode !== "download") return;
    let alive = true;
    listJava()
      .then((result) => {
        if (alive) setRuntimes(result.runtimes);
      })
      .catch(() => {
        if (alive) setRuntimes([]); // the picker still works with "auto"
      });
    return () => {
      alive = false;
    };
  }, [mode, installJobId]);

  // An install job completing: pick up the new managed runtime.
  useEffect(() => {
    if (!installJob) return;
    if (installJob.state === "succeeded") {
      setInstallJobId(null);
      setJavaPath((current) => {
        // Auto-select the freshly fetched runtime once the list lands.
        return current === "auto" ? "auto" : current;
      });
    } else if (installJob.state === "failed" || installJob.state === "cancelled") {
      setInstallJobId(null);
      setSubmitError({
        title: installJob.error?.message ?? "The Java fetch did not finish.",
        code: installJob.error?.code,
        remediation: installJob.error?.remediation ?? [],
      });
    }
  }, [installJob]);

  // The create job completing: open the server, or surface the failure.
  useEffect(() => {
    if (!createJob) return;
    if (createJob.state === "succeeded") {
      void getServer(serverId)
        .then((details) => upsert(details))
        .catch(() => {}); // the registered event reconciles anyway
      navigate({ kind: "server", serverId });
      close(false);
    } else if (createJob.state === "failed" || createJob.state === "cancelled") {
      setCreateJobId(null);
      setBusy(false);
      setSubmitError({
        title:
          createJob.error?.message ??
          (createJob.state === "cancelled"
            ? "The creation was cancelled; nothing was left behind."
            : "The creation did not finish."),
        code: createJob.error?.code,
        remediation: createJob.error?.remediation ?? [],
      });
    }
  }, [createJob, serverId, navigate, close, upsert]);

  const installing = installJobId !== null;
  const needsJava =
    javaMajor !== null && runtimes !== null && !satisfyingRuntime(runtimes, javaMajor);

  const submitDownload = () => {
    const invalid = validateServerId(serverId);
    setIdError(invalid);
    if (invalid) return;
    const port = portText.trim().length > 0 ? Number(portText) : undefined;
    if (port !== undefined && (!Number.isInteger(port) || port < 1 || port > 65535)) {
      setSubmitError({ title: "Port must be a whole number between 1 and 65535.", remediation: [] });
      return;
    }
    // The Fabric family pins a loader instead of a numeric build.
    const isFabric =
      (entries ?? []).find((entry) => entry.id === project)?.source === "fabric-meta";
    setBusy(true);
    setSubmitError(null);
    void createServer({
      serverId,
      displayName: displayName.length > 0 ? displayName : undefined,
      project,
      version,
      build: buildId ?? undefined,
      loader: isFabric && loaderId ? loaderId : undefined,
      port,
      javaPath: javaPath === "auto" ? undefined : javaPath,
    })
      .then((result) => {
        // Seed the store with the reply's own record: the job.started
        // event reconciles the same id, and the modal shows progress
        // even if the event races this reply.
        useJobs.getState().started(result.job);
        setCreateJobId(result.job.jobId);
      })
      .catch((error: unknown) => {
        setSubmitError(describeError(error));
        setBusy(false);
      });
  };

  const submitRegister = () => {
    const invalid = validateServerId(serverId);
    setIdError(invalid);
    if (invalid || rootPath.length === 0) return;
    setBusy(true);
    setSubmitError(null);
    void registerServer({
      serverId,
      displayName: displayName.length > 0 ? displayName : serverId,
      rootPath,
    })
      .then((result) => {
        upsert(result.server);
        navigate({ kind: "server", serverId: result.server.serverId });
        close(false);
      })
      .catch((error: unknown) => {
        setSubmitError(describeError(error));
        setBusy(false);
      });
  };

  const submit = () => (mode === "download" ? submitDownload() : submitRegister());

  const createProgress = createJob?.progress;
  const percent =
    createProgress && createProgress.total && createProgress.total > 0
      ? Math.min(100, Math.round((createProgress.current / createProgress.total) * 100))
      : null;

  return (
    <Modal title="New server" onClose={() => close(false)}>
      <div className={styles.modeSwitch} role="tablist" aria-label="Creation mode">
        <button
          role="tab"
          aria-selected={mode === "download"}
          className={`${styles.modeTab} ${mode === "download" ? styles.modeTabActive : ""}`}
          onClick={() => setMode("download")}
        >
          Download &amp; create
        </button>
        <button
          role="tab"
          aria-selected={mode === "register"}
          className={`${styles.modeTab} ${mode === "register" ? styles.modeTabActive : ""}`}
          onClick={() => setMode("register")}
        >
          Register existing
        </button>
      </div>

      <form
        className={styles.form}
        onSubmit={(event) => {
          event.preventDefault();
          if (createJobId === null) submit();
        }}
      >
        {mode === "download" ? (
          <>
            <div className={styles.field}>
              <label className={styles.label} htmlFor="new-server-software">
                Software
              </label>
              <select
                id="new-server-software"
                className={styles.input}
                value={project}
                onChange={(event) => setProject(event.target.value)}
              >
                {(entries ?? []).map((entry) => (
                  <option key={entry.id} value={entry.id}>
                    {entry.name} — {entry.description}
                  </option>
                ))}
              </select>
              {entries === null ? <span className={styles.hint}>Loading the catalog…</span> : null}
            </div>

            <div className={styles.fieldRow}>
              <div className={styles.field}>
                <label className={styles.label} htmlFor="new-server-version">
                  Version
                </label>
                <select
                  id="new-server-version"
                  className={styles.input}
                  value={version}
                  onChange={(event) => setVersion(event.target.value)}
                >
                  {(versions ?? []).map((v) => (
                    <option key={v.id} value={v.id}>
                      {v.id}
                    </option>
                  ))}
                </select>
              </div>
              <div className={styles.field}>
                {loaders !== null ? (
                  <>
                    <label className={styles.label} htmlFor="new-server-loader">
                      Loader
                    </label>
                    <select
                      id="new-server-loader"
                      className={styles.input}
                      value={loaderId}
                      onChange={(event) => setLoaderId(event.target.value)}
                    >
                      {loaders.map((loader) => (
                        <option key={loader} value={loader}>
                          {loader}
                        </option>
                      ))}
                    </select>
                    <span className={styles.hint}>
                      {loaders.length === 0
                        ? "No stable loader published."
                        : "Stable loaders, newest first."}
                    </span>
                  </>
                ) : (
                  <>
                    <label className={styles.label} htmlFor="new-server-build">
                      Build
                    </label>
                    <select
                      id="new-server-build"
                      className={styles.input}
                      value={buildId === null ? "" : String(buildId)}
                      onChange={(event) => setBuildId(Number(event.target.value))}
                    >
                      {(builds ?? []).map((b) => (
                        <option key={b.id} value={b.id}>
                          #{b.id}
                          {b.channel.toLowerCase() !== "default"
                            ? ` (${b.channel.toLowerCase()})`
                            : ""}
                        </option>
                      ))}
                    </select>
                  </>
                )}
              </div>
            </div>

            <div className={styles.fieldRow}>
              <div className={styles.field}>
                <label className={styles.label} htmlFor="new-server-id">
                  Server id
                </label>
                <input
                  id="new-server-id"
                  className={styles.input}
                  value={serverId}
                  onChange={(event) => setServerId(event.target.value)}
                  placeholder="survival"
                />
                {idError ? <span className={styles.alert}>{idError}</span> : null}
              </div>
              <div className={styles.field}>
                <label className={styles.label} htmlFor="new-server-port">
                  Port <span title="optional">(optional)</span>
                </label>
                <input
                  id="new-server-port"
                  className={styles.input}
                  value={portText}
                  onChange={(event) => setPortText(event.target.value)}
                  placeholder="25565"
                  inputMode="numeric"
                />
              </div>
            </div>

            <div className={styles.field}>
              <label className={styles.label} htmlFor="new-server-name">
                Display name <span title="optional">(optional)</span>
              </label>
              <input
                id="new-server-name"
                className={styles.input}
                value={displayName}
                onChange={(event) => setDisplayName(event.target.value)}
                placeholder="Survival"
              />
            </div>

            <div className={styles.field}>
              <label className={styles.label} htmlFor="new-server-java">
                Java runtime
                {javaMajor !== null ? (
                  <span className={styles.javaChip}> this version needs Java {javaMajor}</span>
                ) : null}
              </label>
              <select
                id="new-server-java"
                className={styles.input}
                value={javaPath}
                onChange={(event) => setJavaPath(event.target.value)}
              >
                <option value="auto">Let the daemon pick</option>
                {(runtimes ?? []).map((runtime) => (
                  <option key={runtime.path} value={runtime.path}>
                    Java {runtime.major} — {runtime.vendor || "unknown vendor"}
                    {runtime.managed ? " (managed)" : ""}
                    {` [${runtime.versionString}]`}
                  </option>
                ))}
              </select>
              {needsJava ? (
                <div className={styles.javaNotice}>
                  <span className={styles.hint}>
                    No discovered runtime satisfies Java {javaMajor}.
                  </span>
                  <Button
                    variant="default"
                    busy={installing}
                    disabled={installing}
                    onClick={() => {
                      // needsJava guarantees the requirement is known.
                      setSubmitError(null);
                      void installJava(javaMajor)
                        .then((result) => {
                          useJobs.getState().started(result.job);
                          setInstallJobId(result.job.jobId);
                        })
                        .catch((error: unknown) => setSubmitError(describeError(error)));
                    }}
                  >
                    Fetch Java {javaMajor} (Eclipse Temurin)
                  </Button>
                </div>
              ) : null}
            </div>

            <span className={styles.hint}>
              The daemon downloads and verifies the server jar, writes the starter files, and
              registers the server. The first start asks for EULA acceptance — nothing else is
              manual.
            </span>
          </>
        ) : (
          <>
            <div className={styles.field}>
              <label className={styles.label} htmlFor="new-server-id-register">
                Server id
              </label>
              <input
                id="new-server-id-register"
                className={styles.input}
                value={serverId}
                onChange={(event) => setServerId(event.target.value)}
                placeholder="survival"
                autoFocus
              />
              {idError ? <span className={styles.alert}>{idError}</span> : null}
            </div>

            <div className={styles.field}>
              <label className={styles.label} htmlFor="new-server-name-register">
                Display name <span title="optional">(optional)</span>
              </label>
              <input
                id="new-server-name-register"
                className={styles.input}
                value={displayName}
                onChange={(event) => setDisplayName(event.target.value)}
                placeholder="Survival"
              />
            </div>

            <div className={styles.field}>
              <label className={styles.label} htmlFor="new-server-root">
                Root directory
              </label>
              <input
                id="new-server-root"
                className={styles.input}
                value={rootPath}
                onChange={(event) => setRootPath(event.target.value)}
                placeholder="/home/you/servers/survival"
              />
              <span className={styles.hint}>
                The directory holding server.jar and eula.txt. This is the one path the protocol
                accepts at registration; afterwards the server is referenced by its id only.
              </span>
            </div>
          </>
        )}

        {createJobId !== null ? (
          <div className={styles.progressBox} role="status">
            <span className={styles.progressLabel}>
              {createJob?.progress?.message ?? "Creating the server…"}
            </span>
            {percent !== null ? (
              <div className={styles.progressTrack}>
                <div className={styles.progressFill} style={{ width: `${percent}%` }} />
              </div>
            ) : (
              <span className={styles.hint}>Working…</span>
            )}
          </div>
        ) : null}

        {submitError ? (
          <div className={styles.alert} role="alert">
            {submitError.title}
            {submitError.code ? ` (${submitError.code})` : ""}
            {submitError.remediation.length > 0 ? (
              <ul className={styles.remediation}>
                {submitError.remediation.map((step) => (
                  <li key={step}>{step}</li>
                ))}
              </ul>
            ) : null}
          </div>
        ) : null}

        <div className={styles.row}>
          <Button onClick={() => close(false)}>Cancel</Button>
          <Button
            variant="primary"
            type="submit"
            busy={busy || createJobId !== null}
            disabled={
              createJobId !== null ||
              serverId.length === 0 ||
              (mode === "register" && rootPath.length === 0) ||
              // Download mode needs something to install: a build (the
              // Fill family) or a loader (the Fabric family) — an empty
              // builds list is the fabric family's normal shape.
              (mode === "download" &&
                (!project ||
                  !version ||
                  ((builds?.length ?? 0) === 0 && (loaders?.length ?? 0) === 0)))
            }
          >
            {mode === "download" ? "Download & create" : "Register"}
          </Button>
        </div>
      </form>
    </Modal>
  );
}
