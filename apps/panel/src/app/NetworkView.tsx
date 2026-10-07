// Network (§37, ADR-0019): desired port vs the boot authority, a live
// bind-test, and the pre-start conflict check. The probe is a read, not
// a guarantee — the final authority is the server binding at boot, and
// the copy says so. Editing the port patches the config model; the
// server.properties bind address is displayed, never written (ADR-0007).

import { useCallback, useEffect, useState } from "react";
import { getNetworkStatus, getServerConfig, setServerConfig } from "../state/actions";
import { describeError } from "../state/errors";
import type { ConfigGetResult, NetworkStatusResult } from "../protocol/types";
import { Button } from "../ui/Button";
import { FieldRow, Rows, StaticRow } from "./configFields";
import styles from "./NetworkView.module.css";

export function NetworkView({ serverId }: { serverId: string }) {
  const [config, setConfig] = useState<ConfigGetResult | null>(null);
  const [status, setStatus] = useState<NetworkStatusResult | null>(null);
  const [portDraft, setPortDraft] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    void Promise.all([getServerConfig(serverId), getNetworkStatus(serverId)])
      .then(([configResult, statusResult]) => {
        setConfig(configResult);
        setStatus(statusResult);
        setPortDraft(configResult.effective.port?.toString() ?? "");
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause).title));
  }, [serverId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const runSavePort = () => {
    setError(null);
    setNotice(null);
    const trimmed = portDraft.trim();
    const patch =
      trimmed === ""
        ? { port: config?.effective.port === undefined ? undefined : null }
        : { port: Number(trimmed) };
    if (patch.port === undefined) {
      setNotice("Nothing changed yet.");
      return;
    }
    if (patch.port !== null && (!Number.isInteger(patch.port) || patch.port <= 0)) {
      setError("The port must be a whole number.");
      return;
    }
    setBusy(true);
    void setServerConfig(serverId, { settings: patch })
      .then((result) => {
        setConfig(result);
        setPortDraft(result.effective.port?.toString() ?? "");
        setNotice("Port saved. The server binds it the next time it starts.");
        return getNetworkStatus(serverId).then(setStatus);
      })
      .catch((cause: unknown) => setError(describeError(cause).title))
      .finally(() => setBusy(false));
  };

  const reprobe = () => {
    setError(null);
    setBusy(true);
    void getNetworkStatus(serverId)
      .then(setStatus)
      .catch((cause: unknown) => setError(describeError(cause).title))
      .finally(() => setBusy(false));
  };

  if (error && !config) {
    return (
      <section className={styles.network} aria-label="Network">
        <div className={styles.alert} role="alert">
          {error}
        </div>
      </section>
    );
  }
  if (!config || !status) {
    return (
      <section className={styles.network} aria-label="Network" aria-busy="true" />
    );
  }

  const probe =
    status.portAvailable === undefined
      ? { cls: styles.dotUnknown, word: "no port to probe" }
      : status.portAvailable
        ? { cls: styles.dotAvailable, word: "available right now" }
        : { cls: styles.dotInUse, word: "in use right now" };

  return (
    <section className={styles.network} aria-label="Network">
      <p className={styles.note}>
        The desired port lives in the config model; <code>server.properties</code> is the
        boot authority and stays Minecraft's file. The availability probe is a
        moment-in-time bind test — the final word is the server binding at boot.
      </p>

      {error ? (
        <div className={styles.alert} role="alert">
          {error}
        </div>
      ) : null}
      {notice && !error ? (
        <div className={styles.notice} role="status">
          {notice}
        </div>
      ) : null}

      <div className={styles.statusLine}>
        <span className={`${styles.dot} ${probe.cls}`} aria-hidden="true" />
        <span>
          Port {status.desiredPort ?? status.propertiesPort ?? "—"}: {probe.word}
        </span>
        <span className={styles.spacer} />
        <Button disabled={busy} onClick={reprobe} title="Run the bind-test again">
          Check again
        </Button>
      </div>

      <Rows>
        <FieldRow
          label="Minecraft port"
          provenance={config.provenance.port}
          onClear={
            config.effective.port === undefined
              ? undefined
              : () => setPortDraft("")
          }
          hint="Players join here. The daemon points server.properties at it at boot."
        >
          <input
            aria-label="Minecraft port"
            type="number"
            min={1024}
            max={65534}
            value={portDraft}
            onChange={(event) => setPortDraft(event.target.value)}
            placeholder="25565"
          />
        </FieldRow>
        <StaticRow
          label="server.properties port"
          value={status.propertiesPort?.toString() ?? "(absent — the server has not booted yet)"}
          hint="The boot authority, read from the server directory."
        />
        <StaticRow
          label="Bind address"
          value={
            status.bindAddress === undefined
              ? "(absent)"
              : status.bindAddress === ""
                ? "0.0.0.0 (all interfaces)"
                : status.bindAddress
          }
          hint="Owned by server.properties (server-ip); edit it through Files."
        />
      </Rows>

      <div>
        <p className={styles.note}>Conflicts — other managed servers desiring the same port:</p>
        {status.conflicts.length === 0 ? (
          <p className={styles.note}>None.</p>
        ) : (
          <ul className={styles.conflicts}>
            {status.conflicts.map((other) => (
              <li key={other}>⚠ {other}</li>
            ))}
          </ul>
        )}
      </div>

      <div className={styles.footer}>
        <Button variant="primary" disabled={busy} onClick={runSavePort}>
          Save port
        </Button>
      </div>
    </section>
  );
}
