// Connection profiles (ADR-0011): which daemon this panel operates. The
// local profile is always there; remote profiles point at a zaminagent
// (host:port + token + pinned certificate fingerprint). Activating a
// profile re-opens the wire — the next handshake carries that token.

import { useState } from "react";
import { LOCAL_PROFILE, useConnections } from "../state/connections";
import type { RemoteProfile } from "../state/connections";
import { reconnectWire } from "../state/wire";
import { useUi } from "../state/ui";
import { Button } from "../ui/Button";
import { Modal } from "../ui/Modal";
import styles from "./ConnectionsModal.module.css";

interface Draft {
  name: string;
  addr: string;
  token: string;
  fingerprint: string;
}

const emptyDraft: Draft = { name: "", addr: "", token: "", fingerprint: "" };

/** Client-side sanity only; the agent answers authoritatively. */
export function validateDraft(draft: Draft): string | null {
  if (draft.name.trim().length === 0) return "A name is required.";
  if (draft.addr.trim().length === 0) return "An address (host:port) is required.";
  if (!/^[\w.-]+:\d{1,5}$/.test(draft.addr.trim())) {
    return "Use host:port, e.g. 203.0.113.7:7443.";
  }
  if (draft.token.length === 0) return "The agent's token is required.";
  if (
    draft.fingerprint.length > 0 &&
    !/^[0-9a-f]{64}$/i.test(draft.fingerprint.replace(/:/g, ""))
  ) {
    return "The fingerprint is 64 hex characters (sha-256).";
  }
  return null;
}

function ConnectionsModalBody() {
  const setConnectionsOpen = useUi((s) => s.setConnectionsOpen);
  const onClose = () => setConnectionsOpen(false);
  const remotes = useConnections((s) => s.remotes);
  const activeId = useConnections((s) => s.activeId);
  const addRemote = useConnections((s) => s.addRemote);
  const removeRemote = useConnections((s) => s.removeRemote);
  const setActive = useConnections((s) => s.setActive);

  const [draft, setDraft] = useState<Draft>(emptyDraft);
  const [formError, setFormError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  const activate = (id: string) => {
    if (id === activeId) {
      onClose();
      return;
    }
    setActive(id);
    reconnectWire();
    onClose();
  };

  const submit = () => {
    const cleaned: Draft = {
      name: draft.name.trim(),
      addr: draft.addr.trim(),
      token: draft.token.trim(),
      fingerprint: draft.fingerprint.replace(/:/g, "").trim().toLowerCase(),
    };
    const error = validateDraft(cleaned);
    if (error) {
      setFormError(error);
      return;
    }
    const id = addRemote(cleaned);
    setActive(id);
    reconnectWire();
    onClose();
  };

  return (
    <Modal title="Connections" onClose={onClose}>
      <div className={styles.body}>
        <ul className={styles.list}>
          <li className={styles.row}>
            <button
              className={`${styles.rowMain} ${activeId === LOCAL_PROFILE.id ? styles.rowActive : ""}`}
              onClick={() => activate(LOCAL_PROFILE.id)}
            >
              <span className={styles.rowName}>{LOCAL_PROFILE.name}</span>
              <span className={styles.rowMeta}>ZIM on this machine</span>
            </button>
          </li>
          {remotes.map((remote) => (
            <RemoteRow
              key={remote.id}
              remote={remote}
              active={activeId === remote.id}
              onActivate={() => activate(remote.id)}
              onRemove={() => removeRemote(remote.id)}
            />
          ))}
        </ul>

        {adding ? (
          <div className={styles.form}>
            <div className={styles.field}>
              <label className={styles.label} htmlFor="conn-name">
                Name
              </label>
              <input
                id="conn-name"
                className={styles.input}
                value={draft.name}
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                placeholder="Hetzner box"
                autoFocus
              />
            </div>
            <div className={styles.field}>
              <label className={styles.label} htmlFor="conn-addr">
                Address
              </label>
              <input
                id="conn-addr"
                className={styles.input}
                value={draft.addr}
                onChange={(e) => setDraft({ ...draft, addr: e.target.value })}
                placeholder="203.0.113.7:7443"
              />
            </div>
            <div className={styles.field}>
              <label className={styles.label} htmlFor="conn-token">
                Token
              </label>
              <input
                id="conn-token"
                className={styles.input}
                type="password"
                value={draft.token}
                onChange={(e) => setDraft({ ...draft, token: e.target.value })}
                placeholder="from the agent's token file"
              />
            </div>
            <div className={styles.field}>
              <label className={styles.label} htmlFor="conn-fingerprint">
                Certificate fingerprint
              </label>
              <input
                id="conn-fingerprint"
                className={styles.input}
                value={draft.fingerprint}
                onChange={(e) => setDraft({ ...draft, fingerprint: e.target.value })}
                placeholder="sha-256 the agent prints at startup (recommended)"
              />
            </div>
            {formError ? <p className={styles.error}>{formError}</p> : null}
            <p className={styles.hint}>
              Without a fingerprint the panel accepts any server certificate —
              pin the one the agent prints.
            </p>
            <div className={styles.formActions}>
              <Button variant="default" onClick={() => setAdding(false)}>
                Cancel
              </Button>
              <Button variant="primary" onClick={submit}>
                Connect
              </Button>
            </div>
          </div>
        ) : (
          <div className={styles.footerRow}>
            <Button variant="default" onClick={() => setAdding(true)}>
              + Add remote
            </Button>
          </div>
        )}
      </div>
    </Modal>
  );
}

function RemoteRow({
  remote,
  active,
  onActivate,
  onRemove,
}: {
  remote: RemoteProfile;
  active: boolean;
  onActivate: () => void;
  onRemove: () => void;
}) {
  return (
    <li className={styles.row}>
      <button
        className={`${styles.rowMain} ${active ? styles.rowActive : ""}`}
        onClick={onActivate}
      >
        <span className={styles.rowName}>{remote.name}</span>
        <span className={styles.rowMeta}>
          {remote.addr}
          {remote.fingerprint ? " · pinned" : " · unpinned"}
        </span>
      </button>
      <button
        className={styles.rowRemove}
        onClick={onRemove}
        aria-label={`Forget ${remote.name}`}
        title="Forget this connection"
      >
        ×
      </button>
    </li>
  );
}

export function ConnectionsModal() {
  const open = useUi((s) => s.connectionsOpen);
  if (!open) return null;
  return <ConnectionsModalBody />;
}
