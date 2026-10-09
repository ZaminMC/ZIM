// Settings (§39, ADR-0019): the server's own identity and behavior —
// distinct from the global ZIM settings page. Edits patch the
// layered model; the reserved rooms (icon, restart policy, crash policy,
// log retention) are stated, not faked, because §82 forbids buttons that
// only visually work.

import { useCallback, useEffect, useState } from "react";
import { getServer, getServerConfig, setServerConfig } from "../state/actions";
import { describeError } from "../state/errors";
import type { DescribedError } from "../state/errors";
import { ErrorNote } from "../ui/ErrorNote";
import { useServers } from "../state/servers";
import type { ConfigGetResult } from "../protocol/types";
import { Button } from "../ui/Button";
import { FieldRow, ReservedRow, Rows, StaticRow } from "./configFields";
import styles from "./SettingsView.module.css";

export function SettingsView({ serverId }: { serverId: string }) {
  const [view, setView] = useState<ConfigGetResult | null>(null);
  const [nameDraft, setNameDraft] = useState("");
  const [keepDraft, setKeepDraft] = useState("");
  const [error, setError] = useState<DescribedError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const upsert = useServers((s) => s.upsert);

  const refresh = useCallback(() => {
    void getServerConfig(serverId)
      .then((result) => {
        setView(result);
        setNameDraft(result.displayName);
        setKeepDraft(result.effective.backupKeep.toString());
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause)));
  }, [serverId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const dirty =
    view !== null &&
    (nameDraft.trim() !== view.displayName ||
      keepDraft.trim() !== view.effective.backupKeep.toString());

  const runSave = () => {
    if (!view) return;
    setError(null);
    setNotice(null);
    const name = nameDraft.trim();
    if (name === "") {
      setError({ title: "The server name must not be empty.", remediation: [] });
      return;
    }
    const keep = Number(keepDraft.trim());
    if (!Number.isInteger(keep) || keep < 1) {
      setError({
          title: "Backup retention must be a whole number of at least 1.",
          remediation: [],
        });
      return;
    }
    const payload: Parameters<typeof setServerConfig>[1] = { settings: {} };
    if (name !== view.displayName) payload.displayName = name;
    if (keep !== view.effective.backupKeep) payload.settings.backupKeep = keep;
    if (!payload.displayName && payload.settings.backupKeep === undefined) {
      setNotice("Nothing changed yet.");
      return;
    }
    setBusy(true);
    void setServerConfig(serverId, payload)
      .then((result) => {
        setView(result);
        setNameDraft(result.displayName);
        setKeepDraft(result.effective.backupKeep.toString());
        setNotice("Saved.");
        // The rename must reach every surface that shows the name —
        // refresh the panel's own server record through the ordinary
        // read path, never by patching two stores by hand.
        return getServer(serverId)
          .then((details) => upsert(details))
          .catch(() => {});
      })
      .catch((cause: unknown) => setError(describeError(cause)))
      .finally(() => setBusy(false));
  };

  if (error && !view) {
    return (
      <section className={styles.settings} aria-label="Server settings">
        <div className={styles.alert} role="alert">
          <ErrorNote error={error} />
        </div>
      </section>
    );
  }
  if (!view) {
    return (
      <section className={styles.settings} aria-label="Server settings" aria-busy="true" />
    );
  }

  const e = view.effective;

  return (
    <section className={styles.settings} aria-label="Server settings">
      <p className={styles.note}>
        This server's own identity and behavior — separate from the panel-wide settings.
        Values marked <em>custom</em> override the global defaults.
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

      <span className={styles.sectionLabel}>Identity</span>
      <Rows>
        <FieldRow label="Server name" hint="Shown in the tab, the header, and listings.">
          <input
            aria-label="Server name"
            value={nameDraft}
            onChange={(event) => setNameDraft(event.target.value)}
          />
        </FieldRow>
        <StaticRow
          label="Join address"
          value={e.port ? `0.0.0.0:${e.port}` : "(no port configured)"}
          hint="What players type; the port is managed on the Network page."
        />
        <ReservedRow
          label="Server icon"
          note="A per-server icon picker is a named future room."
        />
      </Rows>

      <span className={styles.sectionLabel}>Behavior</span>
      <Rows>
        <FieldRow
          label="Backup retention"
          provenance={view.provenance.backupKeep}
          hint="Keep the newest N backups after every successful backup."
        >
          <input
            aria-label="Backup retention count"
            type="number"
            min={1}
            value={keepDraft}
            onChange={(event) => setKeepDraft(event.target.value)}
          />
        </FieldRow>
        <ReservedRow
          label="Restart policy"
          note="Automatic restart-on-exit rules are a named future room."
        />
        <ReservedRow
          label="Crash policy"
          note="Automatic crash triage actions are a named future room; crashes are classified and shown today."
        />
        <ReservedRow
          label="Log retention"
          note="How long archived logs are kept is a named future room."
        />
      </Rows>

      <div className={styles.footer}>
        <Button variant="primary" disabled={busy || !dirty} onClick={runSave}>
          Save changes
        </Button>
      </div>
    </section>
  );
}
