// zaminpanel://settings/ (ADR-0015): an honest internal page. What exists
// is rendered with its real machinery (connections, the daemon identity);
// what is planned is named as planned (§82: unavailable, never pretend).

import { useConnection } from "../../state/connection";
import { activeProfile, useConnections } from "../../state/connections";
import { useUi } from "../../state/ui";
import { Button } from "../../ui/Button";
import styles from "./SettingsPage.module.css";

const RESERVED: Array<{ name: string; note: string }> = [
  { name: "Dutchmen", note: "The agent side of the panel — its rooms (new tab, chats) are reserved." },
  { name: "Bookmarks", note: "Named destinations on a bar under the address bar." },
  { name: "Tab groups", note: "Collapsible groups in the strip." },
  { name: "Extensions", note: "Typed addons with declared permissions." },
  { name: "Publishing", note: "Package and publish a server through provider APIs." },
];

export function SettingsPage() {
  const status = useConnection((s) => s.status);
  const daemon = useConnection((s) => s.daemon);
  const lastError = useConnection((s) => s.lastError);
  const remotes = useConnections((s) => s.remotes);
  const activeId = useConnections((s) => s.activeId);
  const setActive = useConnections((s) => s.setActive);
  const setConnectionsOpen = useUi((s) => s.setConnectionsOpen);
  const profile = activeProfile({ remotes, activeId });
  const statusLabel =
    status === "ready" ? "Online" : status === "connecting" ? "Connecting…" : "Offline";

  return (
    <div className={styles.page}>
      <div className={styles.column}>
        <header className={styles.head}>
          <h1 className={styles.title}>Settings</h1>
          <p className={styles.subtitle}>Panel-local settings. The daemon owns its own.</p>
        </header>

        <section className={styles.section} aria-label="Connections">
          <h2 className={styles.sectionTitle}>Connections</h2>
          <ul className={styles.list}>
            <li className={styles.row}>
              <div className={styles.rowMain}>
                <span className={styles.rowName}>This machine</span>
                <span className={styles.rowDetail}>The local daemon over its socket</span>
              </div>
              {profile.id === "local" ? (
                <span className={styles.activeChip}>Active</span>
              ) : (
                <Button variant="ghost" onClick={() => setActive("local")}>
                  Use
                </Button>
              )}
            </li>
            {remotes.map((remote) => (
              <li key={remote.id} className={styles.row}>
                <div className={styles.rowMain}>
                  <span className={styles.rowName}>{remote.name}</span>
                  <span className={styles.rowDetail}>{remote.addr}</span>
                </div>
                {profile.id === remote.id ? (
                  <span className={styles.activeChip}>Active</span>
                ) : (
                  <Button variant="ghost" onClick={() => setActive(remote.id)}>
                    Use
                  </Button>
                )}
              </li>
            ))}
          </ul>
          <Button variant="ghost" onClick={() => setConnectionsOpen(true)}>
            Manage connections…
          </Button>
        </section>

        <section className={styles.section} aria-label="Daemon">
          <h2 className={styles.sectionTitle}>Daemon</h2>
          <div className={styles.row}>
            <div className={styles.rowMain}>
              <span className={styles.rowName}>
                {daemon ? `${daemon.name} v${daemon.version}` : "zamind"}
              </span>
              <span className={styles.rowDetail} title={lastError ?? undefined}>
                {statusLabel}
                {lastError ? ` — ${lastError}` : ""}
              </span>
            </div>
          </div>
        </section>

        <section className={styles.section} aria-label="Planned">
          <h2 className={styles.sectionTitle}>Planned — not in this build</h2>
          <ul className={styles.reserved}>
            {RESERVED.map((item) => (
              <li key={item.name} className={styles.reservedItem}>
                <span className={styles.reservedName}>{item.name}</span>
                <span className={styles.reservedNote}>{item.note}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>
    </div>
  );
}
