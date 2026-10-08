// zaminpanel://settings/ (ADR-0015): an honest internal page. What exists
// is rendered with its real machinery (connections, the daemon identity,
// the update lane); what is planned is named as planned (§82: unavailable,
// never pretend).

import { useConnection } from "../../state/connection";
import { activeProfile, useConnections } from "../../state/connections";
import { updatesSentence, useUpdates } from "../../state/updates";
import { useUi } from "../../state/ui";
import { Button } from "../../ui/Button";
import styles from "./SettingsPage.module.css";

const RESERVED: Array<{ name: string; note: string }> = [
  { name: "Dutchmen", note: "The agent side of the panel — its rooms (new tab, chats) are reserved." },
  { name: "Tab groups", note: "Collapsible groups in the strip." },
  { name: "Extensions", note: "Typed addons with declared permissions." },
  { name: "Publishing", note: "Package and publish a server through provider APIs." },
];

/** The updates rows (ADR-0024): the lane's phase in plain sentences, the
 *  two automatics as honest toggles, and the manual check. A pending
 *  restart disables the manual check — an install already ran; asking the
 *  channel again would quietly bury that fact (§82). */
function UpdatesSection() {
  const phase = useUpdates((s) => s.phase);
  const prefs = useUpdates((s) => s.prefs);
  const installedVersion = useUpdates((s) => s.installedVersion);
  const setAutoCheck = useUpdates((s) => s.setAutoCheck);
  const setAutoInstall = useUpdates((s) => s.setAutoInstall);
  const check = useUpdates((s) => s.check);
  const installNow = useUpdates((s) => s.installNow);
  const restart = useUpdates((s) => s.restart);
  const dismiss = useUpdates((s) => s.dismiss);

  const busy = phase.kind === "downloading";
  const checkDisabled = busy || phase.kind === "ready";
  const versionLabel = installedVersion
    ? `ZaminPanel v${installedVersion}`
    : "ZaminPanel — development (browser)";

  return (
    <section className={styles.section} aria-label="Updates">
      <h2 className={styles.sectionTitle}>Updates</h2>

      <div className={styles.row}>
        <div className={styles.rowMain}>
          <span className={styles.rowName}>{versionLabel}</span>
          <span className={styles.rowDetail}>{updatesSentence(phase)}</span>
        </div>
        <div className={styles.rowActions}>
          {phase.kind === "available" ? (
            <Button onClick={() => void installNow()}>Install now</Button>
          ) : null}
          {phase.kind === "ready" ? (
            <Button variant="primary" onClick={() => void restart()}>
              Restart now
            </Button>
          ) : null}
          {phase.kind === "checking" || busy ? null : (
            <Button
              variant="ghost"
              disabled={checkDisabled}
              title={
                phase.kind === "ready"
                  ? "A restart is pending — restart first, then check again."
                  : undefined
              }
              onClick={() => void check(true)}
            >
              Check for updates now
            </Button>
          )}
          {phase.kind === "error" || phase.kind === "upToDate" ? (
            <Button variant="ghost" onClick={dismiss}>
              Dismiss
            </Button>
          ) : null}
        </div>
      </div>

      <div className={styles.row}>
        <div className={styles.rowMain}>
          <span className={styles.rowName}>Check for updates automatically</span>
          <span className={styles.rowDetail}>
            On boot and every six hours, against the development channel.
          </span>
        </div>
        <Button
          variant="ghost"
          aria-pressed={prefs.autoCheck}
          onClick={() => setAutoCheck(!prefs.autoCheck)}
        >
          {prefs.autoCheck ? "On" : "Off"}
        </Button>
      </div>

      <div className={styles.row}>
        <div className={styles.rowMain}>
          <span className={styles.rowName}>Install updates automatically</span>
          <span className={styles.rowDetail}>
            Download and install without asking. The restart is always yours to make.
          </span>
        </div>
        <Button
          variant="ghost"
          aria-pressed={prefs.autoInstall}
          onClick={() => setAutoInstall(!prefs.autoInstall)}
        >
          {prefs.autoInstall ? "On" : "Off"}
        </Button>
      </div>

      <div className={styles.row}>
        <div className={styles.rowMain}>
          <span className={styles.rowName}>Channel — Development</span>
          <span className={styles.rowDetail}>
            Installers and the update manifest live in the public{" "}
            <a
              className={styles.channelLink}
              href="https://github.com/ZaminMC/ZaminPanel-Releases/releases"
              target="_blank"
              rel="noreferrer"
            >
              ZaminPanel-Releases
            </a>{" "}
            repo, signed before they are offered.
          </span>
        </div>
      </div>
    </section>
  );
}

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

        <UpdatesSection />

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
