// New Server flow, two doors. The default door is the Phase 6 path:
// pick software from the catalog, pick a version and build, and the
// daemon downloads and registers everything — zero manual JAR handling.
// The other door registers an existing directory (the honest Phase 1–2
// path, kept for servers that already exist on disk). Everything after
// this references the id only (ADR-0004).

import { useEffect, useRef, useState } from "react";
import {
  cancelJob,
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
import {
  formatEta,
  formatSpeed,
  measuredSpeed,
  progressView,
  pushSample,
  stageChecklist,
  type SpeedSample,
} from "../state/creationProgress";
import { useCreationPrefs } from "../state/creationPrefs";
import { sortedServers, useServers, type ServerEntry } from "../state/servers";
import { useTabs } from "../state/tabs";
import { useUi } from "../state/ui";
import type {
  CatalogBuild,
  CatalogEntry,
  JavaRuntime,
} from "../protocol/types";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import { CreationBlock } from "./CreationBlock";
import styles from "./NewServerModal.module.css";

/** The success resolve beat — the block's exit animation window
 *  (§75's --speed-slow plus a frame of settle). Completion itself is
 *  the job's state; this only paces the exit. */
const RESOLVE_BEAT_MS = 300;

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

/** The port occupancy map: every registered server that owns a port —
 *  running, stopped, crashed, all states are valid owners (the registry
 *  is the truth, not the liveness). The new-server form refuses an
 *  occupied port and names the owner, the way the id field refuses a
 *  taken name. */
export function portOwners(entries: ServerEntry[]): Map<number, ServerEntry[]> {
  const owners = new Map<number, ServerEntry[]>();
  for (const server of entries) {
    if (server.port == null) continue;
    const list = owners.get(server.port) ?? [];
    list.push(server);
    owners.set(server.port, list);
  }
  return owners;
}

/** The first free port at or after `start` (the 25565 ladder). */
export function firstFreePort(
  owners: Map<number, unknown>,
  start = 25565,
): number {
  let port = start;
  while (owners.has(port)) port += 1;
  return port;
}

/** The free-port suggestions for the picker's datalist: the first few
 *  rungs of the ladder that no registered server owns. */
export function freePortSuggestions(
  owners: Map<number, unknown>,
  count = 8,
): number[] {
  const suggestions: number[] = [];
  let port = firstFreePort(owners);
  while (suggestions.length < count) {
    suggestions.push(port);
    port = firstFreePort(owners, port + 1);
  }
  return suggestions;
}

export function NewServerModal() {
  const close = useUi((s) => s.setNewServerOpen);
  const navigate = useTabs((s) => s.navigate);
  const upsert = useServers((s) => s.upsert);
  const serverMap = useServers((s) => s.servers);
  // The port picker's occupancy: the registry's own truth, every state a
  // valid owner (the founder's law — like choosing a username, taken is
  // taken whether the server is running or not).
  const owners = portOwners(sortedServers(serverMap));

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
  // The operator's remembered EULA preference (panel-local, persisted;
  // every request carries it explicitly — the daemon never defaults it).
  const autoEulaPref = useCreationPrefs((s) => s.autoEula);
  const setAutoEulaPref = useCreationPrefs((s) => s.setAutoEula);
  const [autoEula, setAutoEula] = useState(autoEulaPref);
  // The measured-speed window over the download's own byte events.
  const samplesRef = useRef<SpeedSample[]>([]);
  // The success resolve beat: REAL completion (the job's own state)
  // paces the block's exit animation, then the dashboard opens.
  const [resolving, setResolving] = useState(false);
  const resolveStartedRef = useRef(false);
  // A cancellation ask is in flight (the button must not double-fire).
  const [cancelling, setCancelling] = useState(false);

  const installJob = useJobs((s) =>
    installJobId ? s.jobs[installJobId] : undefined,
  );
  const createJob = useJobs((s) =>
    createJobId ? s.jobs[createJobId] : undefined,
  );

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
    } else if (
      installJob.state === "failed" ||
      installJob.state === "cancelled"
    ) {
      setInstallJobId(null);
      setSubmitError({
        title: installJob.error?.message ?? "The Java fetch did not finish.",
        code: installJob.error?.code,
        remediation: installJob.error?.remediation ?? [],
      });
    }
  }, [installJob]);

  // The create job completing: the block resolves through its exit beat
  // (real completion paces the animation — never a timer guessing), then
  // the server's dashboard opens. A failure or cancellation surfaces in
  // place: the form unlocks for a retry and nothing stays "breaking".
  useEffect(() => {
    if (!createJob) return;
    if (createJob.state === "succeeded") {
      if (resolveStartedRef.current) return;
      resolveStartedRef.current = true;
      setResolving(true);
      void getServer(serverId)
        .then((details) => upsert(details))
        .catch(() => {}); // the registered event reconciles anyway
      window.setTimeout(() => {
        navigate({ kind: "server", serverId });
        close(false);
      }, RESOLVE_BEAT_MS);
    } else if (
      createJob.state === "failed" ||
      createJob.state === "cancelled"
    ) {
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

  // The speed window: every byte-unit progress event the download
  // subsystem reported becomes a sample; the slope over the last ~3 s is
  // the speed the panel shows. No events, no speed — never a guess.
  useEffect(() => {
    const progress = createJob?.progress;
    if (!progress || progress.unit !== "bytes") return;
    samplesRef.current = pushSample(samplesRef.current, Date.now(), progress.current);
  }, [createJob?.progress]);

  const installing = installJobId !== null;
  const needsJava =
    javaMajor !== null &&
    runtimes !== null &&
    !satisfyingRuntime(runtimes, javaMajor);

  // The port field's live verdict: a parseable port that a registered
  // server owns is refused with the owner's name — occupied is occupied
  // whether the owner is active, inactive, or stopped.
  const portNum =
    portText.trim() === "" || !/^\d+$/.test(portText.trim())
      ? null
      : Number(portText);
  const portConflict =
    portNum !== null && portNum >= 1 && portNum <= 65535
      ? (owners.get(portNum) ?? null)
      : null;
  const portConflictNames = (portConflict ?? [])
    .map((s) => s.displayName)
    .join(", ");

  const submitDownload = () => {
    const invalid = validateServerId(serverId);
    setIdError(invalid);
    if (invalid) return;
    if (portConflict) {
      setSubmitError({
        title: `Port ${portNum} is already owned by ${portConflictNames}.`,
        remediation: [
          `Pick a free port — ${firstFreePort(owners)} is the nearest one.`,
        ],
      });
      return;
    }
    const port = portText.trim().length > 0 ? Number(portText) : undefined;
    if (
      port !== undefined &&
      (!Number.isInteger(port) || port < 1 || port > 65535)
    ) {
      setSubmitError({
        title: "Port must be a whole number between 1 and 65535.",
        remediation: [],
      });
      return;
    }
    // The Fabric family pins a loader instead of a numeric build.
    const isFabric =
      (entries ?? []).find((entry) => entry.id === project)?.source ===
      "fabric-meta";
    setBusy(true);
    setSubmitError(null);
    // A fresh operation starts from a fresh measurement window and a
    // fresh resolve latch — the request itself is the immutable snapshot
    // (built once, from the controls' values at this instant).
    samplesRef.current = [];
    resolveStartedRef.current = false;
    setResolving(false);
    void createServer({
      serverId,
      displayName: displayName.length > 0 ? displayName : undefined,
      project,
      version,
      build: buildId ?? undefined,
      loader: isFabric && loaderId ? loaderId : undefined,
      port,
      javaPath: javaPath === "auto" ? undefined : javaPath,
      autoEula,
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

  const submit = () =>
    mode === "download" ? submitDownload() : submitRegister();

  // The derivation the panel renders: everything comes from the job's own
  // events (state/creationProgress owns the laws; this is display sugar).
  // `locked` is the creation contract: once the request is away, the
  // controls freeze — the operation works from the snapshot it took.
  const locked = createJobId !== null;
  const view = progressView(createJob?.progress);
  const speed = measuredSpeed(samplesRef.current);
  const eta =
    view.bytesTotal != null && view.bytesDone != null && speed !== null
      ? formatEta(view.bytesTotal - view.bytesDone, speed)
      : null;
  const checklist = stageChecklist(view.stage ?? "preparing").filter(
    (stage) => autoEula || stage.id !== "eula",
  );
  const projectLabel =
    (entries ?? []).find((entry) => entry.id === project)?.name ?? project;
  const versionLabel = version || "…";
  const buildLabel =
    loaders !== null
      ? loaderId
        ? `loader ${loaderId}`
        : null
      : buildId !== null
        ? `build #${buildId}`
        : null;
  // The runtime auto-selection resolves against the machine's own list —
  // the same candidates the daemon's selector walks at first start.
  const resolvedJava =
    javaPath === "auto" && javaMajor !== null
      ? satisfyingRuntime(runtimes ?? [], javaMajor)
      : undefined;

  return (
    <Modal title="New server" onClose={() => close(false)}>
      <div
        className={styles.modeSwitch}
        role="tablist"
        aria-label="Creation mode"
      >
        <button
          role="tab"
          aria-selected={mode === "download"}
          className={`${styles.modeTab} ${mode === "download" ? styles.modeTabActive : ""}`}
          disabled={locked}
          onClick={() => setMode("download")}
        >
          Download &amp; create
        </button>
        <button
          role="tab"
          aria-selected={mode === "register"}
          className={`${styles.modeTab} ${mode === "register" ? styles.modeTabActive : ""}`}
          disabled={locked}
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
                disabled={locked}
                onChange={(event) => setProject(event.target.value)}
              >
                {(entries ?? []).map((entry) => (
                  <option key={entry.id} value={entry.id}>
                    {entry.name}
                  </option>
                ))}
              </select>
              {entries === null ? (
                <span className={styles.hint}>Loading the catalog…</span>
              ) : null}
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
                  disabled={locked}
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
                      disabled={locked}
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
                      disabled={locked}
                      onChange={(event) =>
                        setBuildId(Number(event.target.value))
                      }
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
                  disabled={locked}
                  onChange={(event) => setServerId(event.target.value)}
                  placeholder="survival"
                />
                {idError ? (
                  <span className={styles.alert}>{idError}</span>
                ) : null}
              </div>
              <div className={styles.field}>
                <label className={styles.label} htmlFor="new-server-port">
                  Port <span title="optional">(optional)</span>
                </label>
                <input
                  id="new-server-port"
                  className={styles.input}
                  value={portText}
                  disabled={locked}
                  onChange={(event) => setPortText(event.target.value)}
                  placeholder={String(firstFreePort(owners))}
                  inputMode="numeric"
                  list="new-server-port-free"
                  aria-invalid={portConflict ? true : undefined}
                />
                {/* The picker's free rungs: a datalist offers ONLY the
                    ports nobody owns — the dropdown can never suggest a
                    collision (the username-picker law). */}
                <datalist id="new-server-port-free">
                  {freePortSuggestions(owners).map((free) => (
                    <option key={free} value={free} />
                  ))}
                </datalist>
                {portConflict ? (
                  <span className={styles.alert}>
                    Owned by {portConflictNames} — choose a free port.
                  </span>
                ) : null}
                {owners.size > 0 ? (
                  <span className={styles.hint}>
                    {owners.size} port{owners.size === 1 ? "" : "s"} taken:{" "}
                    {[...owners.entries()]
                      .map(
                        ([port, serverList]) =>
                          `:${port} ${serverList.map((s) => s.displayName).join(", ")}`,
                      )
                      .join(", ")}
                  </span>
                ) : (
                  <span className={styles.hint}>
                    Every registered port is free — {firstFreePort(owners)} is
                    yours.
                  </span>
                )}
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
                disabled={locked}
                onChange={(event) => setDisplayName(event.target.value)}
                placeholder="Survival"
              />
            </div>

            <div className={styles.field}>
              <label className={styles.label} htmlFor="new-server-java">
                Java runtime
                {javaMajor !== null ? (
                  <span className={styles.javaChip}>
                    {" "}
                    this version needs Java {javaMajor}
                  </span>
                ) : null}
              </label>
              <select
                id="new-server-java"
                className={styles.input}
                value={javaPath}
                disabled={locked}
                onChange={(event) => setJavaPath(event.target.value)}
              >
                <option value="auto">Select automatically</option>
                {(runtimes ?? []).map((runtime) => (
                  <option key={runtime.path} value={runtime.path}>
                    Java {runtime.major} — {runtime.vendor || "unknown vendor"}
                    {runtime.managed ? " (managed)" : ""}
                    {` [${runtime.versionString}]`}
                  </option>
                ))}
              </select>
              {javaPath === "auto" && !needsJava ? (
                <span className={styles.hint}>
                  {resolvedJava
                    ? `Java ${resolvedJava.major} — selected automatically (${
                        resolvedJava.vendor || "unknown vendor"
                      })`
                    : javaMajor !== null
                      ? `Java ${javaMajor} resolves at the first start${
                          runtimes === null ? " — inspecting this machine's runtimes…" : ""
                        }`
                      : "The runtime resolves at the first start."}
                </span>
              ) : null}
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
                        .catch((error: unknown) =>
                          setSubmitError(describeError(error)),
                        );
                    }}
                  >
                    Fetch Java {javaMajor} (Eclipse Temurin)
                  </Button>
                </div>
              ) : null}
            </div>

            <div className={styles.field}>
              <label className={styles.checkRow} htmlFor="new-server-eula">
                <input
                  id="new-server-eula"
                  type="checkbox"
                  checked={autoEula}
                  disabled={locked}
                  onChange={(event) => {
                    setAutoEula(event.target.checked);
                    setAutoEulaPref(event.target.checked);
                  }}
                />
                <span>Automatically accept the Minecraft EULA</span>
              </label>
              <span className={styles.hint}>
                {autoEula
                  ? "eula.txt is written with eula=true before the first start — this checkbox is your explicit acceptance (aka.ms/minecrafteula), remembered for next time."
                  : "The first start asks for EULA acceptance, as always. Tick the box to accept it now instead — your choice, never a default."}
              </span>
            </div>

            <span className={styles.hint}>
              ZIM downloads and verifies the server jar, writes the starter
              files, and registers the server. The first start asks for EULA
              acceptance unless you tick the auto-accept box above — nothing
              else is manual.
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
                disabled={busy}
                onChange={(event) => setServerId(event.target.value)}
                placeholder="survival"
                autoFocus
              />
              {idError ? <span className={styles.alert}>{idError}</span> : null}
            </div>

            <div className={styles.field}>
              <label
                className={styles.label}
                htmlFor="new-server-name-register"
              >
                Display name <span title="optional">(optional)</span>
              </label>
              <input
                id="new-server-name-register"
                className={styles.input}
                value={displayName}
                disabled={busy}
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
                disabled={busy}
                onChange={(event) => setRootPath(event.target.value)}
                placeholder="/home/you/servers/survival"
              />
              <span className={styles.hint}>
                The directory holding server.jar and eula.txt. This is the one
                path the protocol accepts at registration; afterwards the server
                is referenced by its id only.
              </span>
            </div>
          </>
        )}

        {createJobId !== null ? (
          <div className={styles.progressPanel} role="status">
            <div className={styles.progressHead}>
              <CreationBlock
                view={view}
                succeeded={resolving}
                failed={
                  createJob?.state === "failed" || createJob?.state === "cancelled"
                }
              />
              <div className={styles.progressTitle}>
                <span className={styles.progressName}>
                  Creating {displayName.length > 0 ? displayName : serverId}
                </span>
                <span className={styles.progressConfig}>
                  {projectLabel} · {versionLabel}
                  {buildLabel ? ` · ${buildLabel}` : ""}
                  {resolvedJava || javaPath !== "auto" ? " · " : ""}
                  {resolvedJava
                    ? `Java ${resolvedJava.major} (automatic)`
                    : javaPath !== "auto"
                      ? "Java (pinned)"
                      : ""}
                </span>
              </div>
            </div>

            <span className={styles.progressLabel}>
              {createJob?.progress?.message ?? "Starting the creation…"}
            </span>

            {view.percent !== null ? (
              <div className={styles.progressTrack}>
                <div
                  className={styles.progressFill}
                  style={{ width: `${view.percent}%` }}
                />
              </div>
            ) : (
              /* No honest fraction exists for this stage: a quiet empty
               * track, never a fabricated ramp. */
              <div className={styles.progressTrack} />
            )}

            {view.bytesDone !== null ? (
              <span className={styles.progressNumbers}>
                {formatSpeed(speed) ?? "— B/s"}
                {view.bytesTotal != null
                  ? ` · ${formatProgressBytes(view.bytesDone)} / ${formatProgressBytes(view.bytesTotal)}`
                  : ` · ${formatProgressBytes(view.bytesDone)}`}
                {eta ? ` · ETA ${eta}` : ""}
              </span>
            ) : null}

            <ol className={styles.stageList}>
              {checklist.map((stage) => (
                <li key={stage.id} data-status={stage.status}>
                  <span className={styles.stageGlyph} aria-hidden>
                    {stage.status === "done"
                      ? "✓"
                      : stage.status === "active"
                        ? "●"
                        : "○"}
                  </span>
                  {stage.label}
                </li>
              ))}
            </ol>
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
          {locked ? (
            <Button
              busy={cancelling}
              disabled={cancelling}
              onClick={() => {
                // `locked` narrows: createJobId is a string in here.
                setCancelling(true);
                void cancelJob(createJobId)
                  .catch(() => {
                    // The job's own completion event is the authority —
                    // a refused ask surfaces through the same effect.
                  })
                  .finally(() => setCancelling(false));
              }}
            >
              Cancel creation
            </Button>
          ) : (
            <Button onClick={() => close(false)}>Cancel</Button>
          )}
          <Button
            variant="primary"
            type="submit"
            busy={busy || locked}
            disabled={
              locked ||
              serverId.length === 0 ||
              portConflict != null ||
              (mode === "register" && rootPath.length === 0) ||
              // Download mode needs something to install: a build (the
              // Fill family) or a loader (the Fabric family) — an empty
              // builds list is the fabric family's normal shape.
              (mode === "download" &&
                (!project ||
                  !version ||
                  ((builds?.length ?? 0) === 0 &&
                    (loaders?.length ?? 0) === 0)))
            }
          >
            {mode === "download" ? "Download & create" : "Register"}
          </Button>
        </div>
      </form>
    </Modal>
  );
}
