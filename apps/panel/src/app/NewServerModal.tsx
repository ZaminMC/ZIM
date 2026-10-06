// New Server flow: register an existing directory (the honest path until
// the software catalog lands in Phase 6). The one bootstrap path the
// protocol accepts (ADR-0004); everything after this references the id.

import { useState } from "react";
import { registerServer } from "../state/actions";
import { describeError } from "../state/errors";
import type { DescribedError } from "../state/errors";
import { useServers } from "../state/servers";
import { useUi } from "../state/ui";
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

export function NewServerModal() {
  const close = useUi((s) => s.setNewServerOpen);
  const openServer = useUi((s) => s.openServer);
  const upsert = useServers((s) => s.upsert);

  const [serverId, setServerId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [rootPath, setRootPath] = useState("");
  const [idError, setIdError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<DescribedError | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = () => {
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
        openServer(result.server.serverId);
        close(false);
      })
      .catch((error: unknown) => setSubmitError(describeError(error)))
      .finally(() => setBusy(false));
  };

  return (
    <Modal title="Register a server" onClose={() => close(false)}>
      <form
        className={styles.form}
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
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
            autoFocus
          />
          {idError ? <span className={styles.alert}>{idError}</span> : null}
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
          <Button variant="primary" type="submit" busy={busy} disabled={serverId.length === 0 || rootPath.length === 0}>
            Register
          </Button>
        </div>
      </form>
    </Modal>
  );
}
