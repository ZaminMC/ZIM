// Startup (§38, ADR-0019): the server's launch configuration over the
// layered config model. Every row carries its provenance word; a custom
// row can be cleared back to the global default; saving patches only
// what changed. Overrides are read by the daemon at spawn time, so the
// honest timing note is "the next time the server starts" — the running
// process is never interrupted from this page (§60's rule, held by
// construction).

import { useCallback, useEffect, useState } from "react";
import { getServerConfig, setServerConfig } from "../state/actions";
import { describeError } from "../state/errors";
import type { DescribedError } from "../state/errors";
import { ErrorNote } from "../ui/ErrorNote";
import type { ConfigGetResult, ServerSettingsPatch } from "../protocol/types";
import { Button } from "../ui/Button";
import { FieldRow, Rows } from "./configFields";
import styles from "./StartupView.module.css";

/** One editable field's draft state: its text, plus whether the operator
 *  cleared the override entirely. */
interface Draft {
  text: string;
  cleared: boolean;
}

function draftOf(value: string | undefined | null): Draft {
  return { text: value ?? "", cleared: false };
}

/** The baseline a save diffes against: the effective view as it rendered.
 *  A field the operator never touched must NOT become an override — an
 *  inherited global value shown in the form is not the operator's choice. */
type Baseline = Record<string, string>;

function adoptFrom(result: ConfigGetResult): { drafts: Record<string, Draft>; baseline: Baseline } {
  const drafts: Record<string, Draft> = {
    jar: draftOf(result.jar),
    javaPath: draftOf(result.effective.javaPath),
    minMemoryMb: draftOf(result.effective.minMemoryMb?.toString()),
    maxMemoryMb: draftOf(result.effective.maxMemoryMb?.toString()),
    extraJvmArgs: {
      text: result.effective.extraJvmArgs.join("\n"),
      cleared: false,
    },
    stopTimeoutSecs: draftOf(result.effective.stopTimeoutSecs.toString()),
    startupTimeoutSecs: draftOf(result.effective.startupTimeoutSecs.toString()),
    mcVersion: draftOf(result.effective.mcVersion),
    javaMajorRequired: draftOf(result.effective.javaMajorRequired?.toString()),
  };
  const baseline: Baseline = {};
  for (const [field, draft] of Object.entries(drafts)) {
    baseline[field] = draft.text;
  }
  return { drafts, baseline };
}

export function StartupView({ serverId }: { serverId: string }) {
  const [view, setView] = useState<ConfigGetResult | null>(null);
  const [error, setError] = useState<DescribedError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [baseline, setBaseline] = useState<Baseline>({});

  const refresh = useCallback(() => {
    void getServerConfig(serverId)
      .then((result) => {
        setView(result);
        setError(null);
        const adopted = adoptFrom(result);
        setDrafts(adopted.drafts);
        setBaseline(adopted.baseline);
      })
      .catch((cause: unknown) => setError(describeError(cause)));
  }, [serverId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const setDraft = (field: string, next: Draft) =>
    setDrafts((current) => ({ ...current, [field]: next }));

  const runSave = () => {
    if (!view) return;
    setError(null);
    setNotice(null);
    const settings: ServerSettingsPatch = {};
    try {
      // Tri-state diffing against the rendered baseline: cleared → null
      // (drop the override), text unchanged from the baseline → absent
      // (keep, never turn an inherited global into an override), a new
      // value → the parsed override, deleted text → null (an emptied
      // field reads as "no override here").
      const num = (field: string, label: string): number | null | undefined => {
        const draft = drafts[field];
        if (!draft) return undefined;
        if (draft.cleared) return null;
        const text = draft.text.trim();
        if (text === (baseline[field] ?? "")) return undefined;
        if (text === "") return null;
        const value = Number(text);
        if (!Number.isFinite(value) || value < 0 || !Number.isInteger(value)) {
          throw new Error(`${label} must be a whole number.`);
        }
        return value;
      };
      const str = (field: string): string | null | undefined => {
        const draft = drafts[field];
        if (!draft) return undefined;
        if (draft.cleared) return null;
        const text = draft.text.trim();
        if (text === (baseline[field] ?? "")) return undefined;
        if (text === "") return null;
        return text;
      };
      const args = (): string[] | null | undefined => {
        const draft = drafts.extraJvmArgs;
        if (!draft) return undefined;
        if (draft.cleared) return null;
        const lines = draft.text
          .split("\n")
          .map((line) => line.trim())
          .filter((line) => line !== "");
        if (lines.join("\n") === (baseline.extraJvmArgs ?? "")) return undefined;
        return lines;
      };

      const min = num("minMemoryMb", "The minimum memory");
      if (min !== undefined) settings.minMemoryMb = min;
      const max = num("maxMemoryMb", "The maximum memory");
      if (max !== undefined) settings.maxMemoryMb = max;
      const stop = num("stopTimeoutSecs", "The stop timeout");
      if (stop !== undefined) settings.stopTimeoutSecs = stop;
      const startup = num("startupTimeoutSecs", "The startup window");
      if (startup !== undefined) settings.startupTimeoutSecs = startup;
      const major = num("javaMajorRequired", "The Java major");
      if (major !== undefined) settings.javaMajorRequired = major;
      const javaPath = str("javaPath");
      if (javaPath !== undefined) settings.javaPath = javaPath;
      const mcVersion = str("mcVersion");
      if (mcVersion !== undefined) settings.mcVersion = mcVersion;
      const jvmArgs = args();
      if (jvmArgs !== undefined) settings.extraJvmArgs = jvmArgs;
      const jarPatch = str("jar");

      const payload = {
        settings,
        ...(jarPatch !== undefined ? { jar: jarPatch } : {}),
      };
      if (Object.keys(settings).length === 0 && payload.jar === undefined) {
        setNotice("Nothing changed yet.");
        return;
      }
      setBusy(true);
      void setServerConfig(serverId, payload)
        .then((result) => {
          setNotice("Saved. Overrides apply the next time the server starts.");
          setView(result);
          const adopted = adoptFrom(result);
          setDrafts(adopted.drafts);
          setBaseline(adopted.baseline);
        })
        .catch((cause: unknown) => setError(describeError(cause)))
        .finally(() => setBusy(false));
    } catch (cause) {
      setError({ title: cause instanceof Error ? cause.message : String(cause), remediation: [] });
    }
  };

  if (error && !view) {
    return (
      <section className={styles.startup} aria-label="Startup">
        <div className={styles.alert} role="alert">
        <ErrorNote error={error} />
      </div>
      </section>
    );
  }
  if (!view) {
    return (
      <section className={styles.startup} aria-label="Startup" aria-busy="true" />
    );
  }

  const e = view.effective;
  const p = view.provenance;
  const java = e.javaPath?.trim() || "<best managed runtime>";
  const jarName = drafts.jar?.text.trim() || "server.jar (built-in default)";
  const commandParts = [
    java,
    drafts.minMemoryMb?.text.trim() ? `-Xms${drafts.minMemoryMb.text.trim()}M` : null,
    drafts.maxMemoryMb?.text.trim() ? `-Xmx${drafts.maxMemoryMb.text.trim()}M` : null,
    ...(drafts.extraJvmArgs?.text ?? "")
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line !== ""),
    "-jar",
    jarName,
    "nogui",
  ].filter((part): part is string => part !== null);

  return (
    <section className={styles.startup} aria-label="Startup">
      <p className={styles.note}>
        How this server launches. Values marked <em>custom</em> override the global
        defaults; the × drops the override. Changes apply the next time the server starts —
        a running process is never interrupted from here.
      </p>

      {error ? (
        <div className={styles.alert} role="alert">
        <ErrorNote error={error} />
      </div>
      ) : null}
      {notice && !error ? (
        <div className={styles.notice} role="status">
          {notice}
        </div>
      ) : null}

      <Rows>
        <FieldRow
          label="Server JAR"
          onClear={view.jar ? () => setDraft("jar", { text: "", cleared: true }) : undefined}
          hint="Server-root relative; empty uses the built-in server.jar."
        >
          <input
            aria-label="Server JAR"
            value={drafts.jar?.text ?? ""}
            onChange={(event) => setDraft("jar", { text: event.target.value, cleared: false })}
            placeholder="server.jar"
          />
        </FieldRow>
        <FieldRow
          label="Java executable"
          provenance={p.javaPath}
          onClear={
            e.javaPath ? () => setDraft("javaPath", { text: "", cleared: true }) : undefined
          }
          hint="Empty picks the best managed runtime for this server's Java requirement."
        >
          <input
            aria-label="Java executable"
            value={drafts.javaPath?.text ?? ""}
            onChange={(event) =>
              setDraft("javaPath", { text: event.target.value, cleared: false })
            }
            placeholder="<best managed runtime>"
          />
        </FieldRow>
        <FieldRow
          label="Minimum memory (MiB)"
          provenance={p.minMemoryMb}
          onClear={
            e.minMemoryMb === undefined
              ? undefined
              : () => setDraft("minMemoryMb", { text: "", cleared: true })
          }
          hint="The -Xms heap floor."
        >
          <input
            aria-label="Minimum memory in MiB"
            type="number"
            min={16}
            value={drafts.minMemoryMb?.text ?? ""}
            onChange={(event) =>
              setDraft("minMemoryMb", { text: event.target.value, cleared: false })
            }
          />
        </FieldRow>
        <FieldRow
          label="Maximum memory (MiB)"
          provenance={p.maxMemoryMb}
          onClear={
            e.maxMemoryMb === undefined
              ? undefined
              : () => setDraft("maxMemoryMb", { text: "", cleared: true })
          }
          hint="The -Xmx heap ceiling."
        >
          <input
            aria-label="Maximum memory in MiB"
            type="number"
            min={16}
            value={drafts.maxMemoryMb?.text ?? ""}
            onChange={(event) =>
              setDraft("maxMemoryMb", { text: event.target.value, cleared: false })
            }
          />
        </FieldRow>
        <FieldRow
          label="Extra JVM arguments"
          provenance={p.extraJvmArgs}
          onClear={
            e.extraJvmArgs.length === 0
              ? undefined
              : () => setDraft("extraJvmArgs", { text: "", cleared: true })
          }
          hint="One argument per line, e.g. -XX:+UseG1GC"
        >
          <textarea
            aria-label="Extra JVM arguments, one per line"
            value={drafts.extraJvmArgs?.text ?? ""}
            onChange={(event) =>
              setDraft("extraJvmArgs", { text: event.target.value, cleared: false })
            }
            placeholder={"-XX:+UseG1GC\n-XX:MaxGCPauseMillis=50"}
          />
        </FieldRow>
        <FieldRow
          label="Stop timeout (secs)"
          provenance={p.stopTimeoutSecs}
          onClear={undefined}
          hint="How long a graceful stop may take before the ladder escalates."
        >
          <input
            aria-label="Stop timeout in seconds"
            type="number"
            min={1}
            value={drafts.stopTimeoutSecs?.text ?? ""}
            onChange={(event) =>
              setDraft("stopTimeoutSecs", { text: event.target.value, cleared: false })
            }
          />
        </FieldRow>
        <FieldRow
          label="Startup window (secs)"
          provenance={p.startupTimeoutSecs}
          onClear={undefined}
          hint="How long the daemon waits for the Done line before calling it a startup failure."
        >
          <input
            aria-label="Startup window in seconds"
            type="number"
            min={1}
            value={drafts.startupTimeoutSecs?.text ?? ""}
            onChange={(event) =>
              setDraft("startupTimeoutSecs", { text: event.target.value, cleared: false })
            }
          />
        </FieldRow>
        <FieldRow
          label="Minecraft version"
          provenance={p.mcVersion}
          onClear={
            e.mcVersion === undefined
              ? undefined
              : () => setDraft("mcVersion", { text: "", cleared: true })
          }
          hint="Drives the required Java major when no direct override is set."
        >
          <input
            aria-label="Minecraft version"
            value={drafts.mcVersion?.text ?? ""}
            onChange={(event) =>
              setDraft("mcVersion", { text: event.target.value, cleared: false })
            }
            placeholder="1.21.1"
          />
        </FieldRow>
        <FieldRow
          label="Required Java major"
          provenance={p.javaMajorRequired}
          onClear={
            e.javaMajorRequired === undefined
              ? undefined
              : () => setDraft("javaMajorRequired", { text: "", cleared: true })
          }
          hint="Direct override; wins over the version-derived requirement."
        >
          <input
            aria-label="Required Java major"
            type="number"
            min={8}
            value={drafts.javaMajorRequired?.text ?? ""}
            onChange={(event) =>
              setDraft("javaMajorRequired", { text: event.target.value, cleared: false })
            }
          />
        </FieldRow>
      </Rows>

      <div>
        <span className={styles.commandLabel}>Next start would run</span>
        <div className={styles.command}>{commandParts.join(" ")}</div>
      </div>

      <div className={styles.footer}>
        <Button variant="primary" disabled={busy} onClick={runSave}>
          Save changes
        </Button>
      </div>
    </section>
  );
}
