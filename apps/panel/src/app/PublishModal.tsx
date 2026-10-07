// The publish workspace (founder §40–47, ADR-0017): the §40 form
// (provider, title, description, version, changelog, selection), the
// §42 diff with its change counter, the §44/§46 security panel with the
// four verbs (Exclude File / Review / Publish Anyway / Cancel), and the
// §74 job progress. The §43 Dutchmen changelog room is present as a
// named-and-disabled button — the founder's "keep a room for it", not a
// fake feature.

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  executePublish,
  getPublishState,
  listPublishProviders,
  previewPublish,
  setPublishConfig,
  setPublishReview,
} from "../state/actions";
import { describeError } from "../state/errors";
import { useJobs } from "../state/jobs";
import type {
  FileDiffEntry,
  FileDiffStatus,
  PublishConfig,
  PublishPreviewResult,
  ProviderInfo,
  SelectionRule,
  SecretFinding,
} from "../protocol/types";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import styles from "./PublishModal.module.css";

export function ruleToText(rule: SelectionRule): string {
  if (rule.kind === "folder") return `folder:${rule.path}`;
  if (rule.kind === "file") return `file:${rule.path}`;
  return `glob:${rule.pattern}`;
}

export function parseRuleText(text: string): SelectionRule | null {
  const colon = text.indexOf(":");
  if (colon <= 0) return null;
  const kind = text.slice(0, colon).trim().toLowerCase();
  const payload = text.slice(colon + 1).trim();
  if (!payload) return null;
  if (kind === "folder") return { kind: "folder", path: payload };
  if (kind === "file") return { kind: "file", path: payload };
  if (kind === "glob") return { kind: "glob", pattern: payload };
  return null;
}

export function statusMark(status: FileDiffStatus): string {
  if (status === "added") return "A";
  if (status === "modified") return "M";
  if (status === "removed") return "D";
  return "·";
}

/** Where the §42 emphasis lives: changes exist → the chip lights up. */
export function changedChipClass(count: number): string {
  return (count > 0 ? styles.changedEmph : styles.changedQuiet) ?? "";
}

function findingWhere(finding: SecretFinding): string {
  return finding.line > 0 ? `${finding.file}:${finding.line}` : finding.file;
}

export function PublishModal({
  serverId,
  serverName,
  onClose,
}: {
  serverId: string;
  serverName: string;
  onClose: () => void;
}) {
  const [providers, setProviders] = useState<ProviderInfo[] | null>(null);
  const [config, setConfig] = useState<PublishConfig | null>(null);
  const [preview, setPreview] = useState<PublishPreviewResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [ruleDraft, setRuleDraft] = useState("");
  // §45: the security check panel opens on a blocked publish attempt and
  // Publish Anyway needs the explicit second click, never a stray one.
  const [securityOpen, setSecurityOpen] = useState(false);
  const [anywayArmed, setAnywayArmed] = useState(false);

  const jobs = useJobs((s) => s.jobs);
  const runningJob = useMemo(
    () =>
      Object.values(jobs).find(
        (job) => job.serverId === serverId && job.kind === "publish.execute" && (job.state === "running" || job.state === "queued"),
      ),
    [jobs, serverId],
  );
  const stage = runningJob?.progress?.message ?? null;

  const refresh = useCallback(() => {
    void previewPublish(serverId)
      .then((result) => {
        setPreview(result);
        setConfig(result.config);
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause).title));
  }, [serverId]);

  useEffect(() => {
    void listPublishProviders()
      .then((result) => setProviders(result.providers))
      .catch((cause: unknown) => setError(describeError(cause).title));
    refresh();
  }, [refresh]);

  if (!config || !preview) {
    return (
      <Modal title={`Publish ${serverName}`} onClose={onClose}>
        {error ? (
          <p className={styles.error} role="alert">
            {error}
          </p>
        ) : (
          <p className={styles.quiet}>Reading the publish state…</p>
        )}
      </Modal>
    );
  }

  const provider = providers?.find((p) => p.id === config.providerId) ?? null;
  const outDir = config.providerSettings["outDir"] ?? "";

  const save = async (next: PublishConfig) => {
    setBusy(true);
    try {
      const saved = await setPublishConfig(serverId, next);
      setConfig(saved);
      setNotice("Configuration saved.");
      refresh();
    } catch (cause: unknown) {
      setError(describeError(cause).title);
    } finally {
      setBusy(false);
    }
  };

  const patch = (part: Partial<PublishConfig>) => {
    const next = { ...config, ...part };
    setConfig(next);
    void save(next);
  };

  const addRule = (list: "includes" | "excludes") => {
    const rule = parseRuleText(ruleDraft);
    if (!rule) {
      setError("A rule reads folder:PATH, file:PATH, or glob:PATTERN.");
      return;
    }
    setError(null);
    const next = {
      ...config,
      selection: {
        includes:
          list === "includes" ? [...config.selection.includes, rule] : config.selection.includes,
        excludes:
          list === "excludes" ? [...config.selection.excludes, rule] : config.selection.excludes,
      },
    };
    setRuleDraft("");
    void save(next);
  };

  const removeRule = (list: "includes" | "excludes", index: number) => {
    const kept = config.selection[list].filter((_, i) => i !== index);
    void save({ ...config, selection: { ...config.selection, [list]: kept } });
  };

  const excludeFile = (finding: SecretFinding) => {
    const rule: SelectionRule = { kind: "file", path: finding.file };
    if (config.selection.excludes.some((r) => ruleToText(r) === ruleToText(rule))) return;
    void save({
      ...config,
      selection: { ...config.selection, excludes: [...config.selection.excludes, rule] },
    });
  };

  const review = (finding: SecretFinding) => {
    void setPublishReview(serverId, finding.file, finding.kind, true)
      .then((result) => {
        setPreview(result);
        setSecurityOpen(false);
        setAnywayArmed(false);
      })
      .catch((cause: unknown) => setError(describeError(cause).title));
  };

  const run = async (confirmUnsafe: boolean) => {
    setBusy(true);
    setError(null);
    setSecurityOpen(false);
    setAnywayArmed(false);
    try {
      await executePublish(serverId, confirmUnsafe);
      setNotice("Publishing — the job walks preparing, scanning, packaging, uploading.");
    } catch (cause: unknown) {
      const described = describeError(cause);
      if (described.code === "PUBLISH_SECRETS_DETECTED") {
        // The §45 security check opens INSTEAD of a dead-end error.
        setSecurityOpen(true);
        setAnywayArmed(false);
      } else {
        setError(described.title);
      }
      refresh();
    } finally {
      setBusy(false);
    }
  };

  const diffRows: FileDiffEntry[] = preview.files;

  return (
    <Modal title={`Publish ${serverName}`} onClose={onClose}>
      <div className={styles.body} data-testid="publish-modal">
        {error ? (
          <p className={styles.error} role="alert">
            {error}
          </p>
        ) : null}
        {notice ? (
          <p className={styles.notice} role="status">
            {notice}
          </p>
        ) : null}
        {runningJob ? (
          <p className={styles.notice} role="status" data-testid="publish-stage">
            {stage ?? "Working…"}
          </p>
        ) : null}

        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>What changed</h3>
          <span className={changedChipClass(preview.counts.changed)} data-testid="changed-chip">
            {preview.counts.changed} file{preview.counts.changed === 1 ? "" : "s"} changed
          </span>
          {diffRows.length > 0 ? (
            <table className={styles.diffTable}>
              <thead>
                <tr>
                  <th aria-label="status" />
                  <th>File</th>
                  <th>Size</th>
                </tr>
              </thead>
              <tbody>
                {diffRows.map((entry) => (
                  <tr key={entry.path} className={styles[entry.status] ?? undefined}>
                    <td className={styles.mark}>{statusMark(entry.status)}</td>
                    <td>{entry.path}</td>
                    <td>{entry.size ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <p className={styles.quiet}>
              Nothing is selected yet — add an include rule below. The whole server directory is
              never packaged.
            </p>
          )}
        </section>

        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>Security scan</h3>
          <p className={styles.quiet}>
            A safety mechanism, not a guarantee. Excerpts are redacted — the secret itself is never
            shown.
          </p>
          {preview.scan.findings.length === 0 ? (
            <p className={styles.quiet}>No findings.</p>
          ) : (
            <ul className={styles.findings}>
              {preview.scan.findings.map((finding) => (
                <li
                  key={`${finding.file}:${finding.line}:${finding.kind}`}
                  className={styles[finding.severity] ?? undefined}
                >
                  <span className={styles.findingHead}>
                    {finding.severity} · {finding.kind}
                    {finding.reviewed ? " · reviewed" : ""}
                  </span>
                  <span className={styles.findingWhere}>{findingWhere(finding)}</span>
                  <span className={styles.findingExcerpt}>{finding.excerpt}</span>
                  <span className={styles.findingActions}>
                    <Button variant="ghost" onClick={() => excludeFile(finding)}>
                      Exclude file
                    </Button>
                    {finding.reviewed ? null : (
                      <Button variant="ghost" onClick={() => review(finding)}>
                        Review
                      </Button>
                    )}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {preview.blockingCount > 0 ? (
            <p className={styles.blocking} data-testid="blocking-count">
              {preview.blockingCount} finding{preview.blockingCount === 1 ? "" : "s"} would refuse
              the publish.
            </p>
          ) : null}
        </section>

        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>Selection</h3>
          <div className={styles.ruleRows}>
            {config.selection.includes.map((rule, index) => (
              <span key={`in-${ruleToText(rule)}-${index}`} className={styles.ruleRow}>
                <code>{ruleToText(rule)}</code>
                <Button variant="ghost" onClick={() => removeRule("includes", index)}>
                  Remove
                </Button>
              </span>
            ))}
            {config.selection.excludes.map((rule, index) => (
              <span key={`ex-${ruleToText(rule)}-${index}`} className={styles.ruleRow}>
                <code>not {ruleToText(rule)}</code>
                <Button variant="ghost" onClick={() => removeRule("excludes", index)}>
                  Remove
                </Button>
              </span>
            ))}
          </div>
          <div className={styles.ruleAdd}>
            <input
              className={styles.ruleInput}
              placeholder="folder:plugins/TAB · file:server.properties · glob:plugins/**/*.yml"
              value={ruleDraft}
              onChange={(event) => setRuleDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  addRule("includes");
                }
              }}
            />
            <Button variant="ghost" onClick={() => addRule("includes")}>
              Include
            </Button>
            <Button variant="ghost" onClick={() => addRule("excludes")}>
              Exclude
            </Button>
          </div>
        </section>

        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>Package</h3>
          <label className={styles.field}>
            Provider
            <select
              className={styles.input}
              value={config.providerId}
              onChange={(event) => patch({ providerId: event.target.value, providerSettings: {} })}
            >
              {(providers ?? []).map((p) => (
                <option key={p.id} value={p.id}>
                  {p.displayName}
                </option>
              ))}
            </select>
          </label>
          {provider?.settings.some((setting) => setting.key === "outDir") ? (
            <label className={styles.field}>
              Output folder (absolute)
              <input
                className={styles.input}
                value={outDir}
                onChange={(event) =>
                  patch({
                    providerSettings: { ...config.providerSettings, outDir: event.target.value },
                  })
                }
              />
            </label>
          ) : null}
          <label className={styles.field}>
            Title
            <input
              className={styles.input}
              placeholder="the server id is used when blank"
              value={config.title}
              onChange={(event) => setConfig({ ...config, title: event.target.value })}
              onBlur={(event) => patch({ title: event.target.value })}
            />
          </label>
          <label className={styles.field}>
            Version
            <input
              className={styles.input}
              value={config.version}
              onChange={(event) => setConfig({ ...config, version: event.target.value })}
              onBlur={(event) => patch({ version: event.target.value })}
            />
          </label>
          <label className={styles.field}>
            Description
            <textarea
              className={styles.input}
              rows={2}
              value={config.description}
              onChange={(event) => setConfig({ ...config, description: event.target.value })}
              onBlur={(event) => patch({ description: event.target.value })}
            />
          </label>
          <label className={styles.field}>
            Changelog
            <textarea
              className={styles.input}
              rows={3}
              placeholder="what actually changed since the last publication"
              value={config.changelog}
              onChange={(event) => setConfig({ ...config, changelog: event.target.value })}
              onBlur={(event) => patch({ changelog: event.target.value })}
            />
          </label>
          <Button
            variant="ghost"
            disabled
            title="Reserved: the Dutchmen changelog room (founder §43). Named, not faked."
            onClick={() => undefined}
          >
            Generate with Dutchmen — reserved
          </Button>
        </section>

        {securityOpen ? (
          <section className={styles.security} data-testid="security-check" role="alertdialog" aria-label="Security check">
            <h3 className={styles.sectionTitle}>Security check</h3>
            <p className={styles.blocking}>
              Potential secret detected — review the findings, exclude the files, or publish
              anyway explicitly.
            </p>
            <ul className={styles.findings}>
              {preview.scan.findings
                .filter((finding) => !finding.reviewed && finding.severity !== "low")
                .map((finding) => (
                  <li key={`sec-${finding.file}:${finding.line}:${finding.kind}`}>
                    <span className={styles.findingHead}>
                      {finding.severity} · {finding.kind}
                    </span>
                    <span className={styles.findingWhere}>{findingWhere(finding)}</span>
                    <span className={styles.findingActions}>
                      <Button variant="ghost" onClick={() => excludeFile(finding)}>
                        Exclude file
                      </Button>
                      <Button variant="ghost" onClick={() => review(finding)}>
                        Review
                      </Button>
                    </span>
                  </li>
                ))}
            </ul>
            <div className={styles.securityActions}>
              {anywayArmed ? (
                <Button
                  variant="danger"
                  disabled={busy}
                  data-testid="publish-anyway-confirm"
                  onClick={() => void run(true)}
                >
                  Publish anyway — I understand
                </Button>
              ) : (
                <Button
                  variant="default"
                  disabled={busy}
                  data-testid="publish-anyway-arm"
                  onClick={() => setAnywayArmed(true)}
                >
                  Publish anyway…
                </Button>
              )}
              <Button variant="ghost" onClick={() => setSecurityOpen(false)}>
                Cancel
              </Button>
            </div>
          </section>
        ) : null}

        <footer className={styles.footer}>
          <Button
            variant="primary"
            disabled={busy || runningJob !== undefined || preview.selectedFiles === 0}
            title={
              preview.selectedFiles === 0
                ? "Nothing is selected; add include rules first"
                : undefined
            }
            data-testid="publish-run"
            onClick={() => {
              if (preview.blockingCount > 0) {
                setSecurityOpen(true);
                setAnywayArmed(false);
                return;
              }
              void run(false);
            }}
          >
            Publish
          </Button>
          <Button
            variant="ghost"
            onClick={() => {
              void getPublishState(serverId)
                .then((state) => {
                  if (state.lastPublication) {
                    setNotice(
                      `Last published ${new Date(state.lastPublication.publishedAtMs).toLocaleString()} via ${state.lastPublication.providerId}.`,
                    );
                  } else {
                    setNotice("Never published.");
                  }
                })
                .catch(() => undefined);
            }}
          >
            Last publication…
          </Button>
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
        </footer>
      </div>
    </Modal>
  );
}
