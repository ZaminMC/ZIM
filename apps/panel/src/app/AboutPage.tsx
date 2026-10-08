// zaminpanel://about/ — the about page (§58, ADR-0026). The version the
// host actually answered for, the update channel the install rides, the
// daemon on the other end of the wire, and the rooms the panel has not
// built yet — stated as reserved, not faked (§82). No telemetry, no
// phone-home beyond the update check the settings already govern.

import { useConnection } from "../state/connection";
import { useUpdates } from "../state/updates";
import styles from "./AboutPage.module.css";

export function AboutPage() {
  const installedVersion = useUpdates((s) => s.installedVersion);
  const daemon = useConnection((s) => s.daemon);
  const status = useConnection((s) => s.status);

  return (
    <div className={styles.page}>
      <header className={styles.head}>
        <h1 className={styles.title}>ZaminPanel</h1>
        <p className={styles.tagline}>A browser for Minecraft servers.</p>
      </header>

      <section className={styles.section} aria-label="Version">
        <h2 className={styles.sectionTitle}>Version</h2>
        <dl className={styles.facts}>
          <dt>Installed</dt>
          <dd>
            {installedVersion ?? "the host has not answered yet"}
          </dd>
          <dt>Channel</dt>
          <dd>Development — updates arrive from the dev releases lane, signed and verified</dd>
          {daemon ? (
            <>
              <dt>Daemon</dt>
              <dd>
                {daemon.name} v{daemon.version}
                {status === "ready" ? "" : " (not connected)"}
              </dd>
            </>
          ) : null}
        </dl>
        <p className={styles.note}>
          Update checks run at boot and every six hours while "check automatically" is on; the
          Settings page owns those preferences. An offered update installs itself when "install
          automatically" is on, and the panel waits — it never restarts the session on its own.
        </p>
      </section>

      <section className={styles.section} aria-label="Reserved rooms">
        <h2 className={styles.sectionTitle}>Reserved rooms</h2>
        <p className={styles.body}>
          Surfaces the founder's design holds seats for, which arrive with their real machinery
          rather than a placeholder pretending to work:
        </p>
        <ul className={styles.rooms}>
          <li>
            <strong>Dutchmen</strong> — the panel's agent (§66–72). The crash card's "Ask
            Dutchmen" and the strip's "Share tab with Dutchmen" are its labeled seats.
          </li>
          <li>
            <strong>Extensions</strong> — typed addons with declared permissions (§56–57). The
            specialized-editor registry is the first seam they will plug into.
          </li>
          <li>
            <strong>Conversation destinations</strong> — the <code>dutchmen:&lt;id&gt;</code>{" "}
            address dialect and bookmark kind grow with the agent runtime (§58, §55).
          </li>
        </ul>
      </section>

      <section className={styles.section} aria-label="License">
        <h2 className={styles.sectionTitle}>License</h2>
        <p className={styles.body}>
          Apache License 2.0. The panel is a client; the daemon owns the processes, the files,
          and the audit trail (§65 — one authoritative backend).
        </p>
      </section>
    </div>
  );
}

