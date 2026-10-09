// zim://settings/ (ADR-0015): an honest internal page. What exists
// is rendered with its real machinery (connections, the daemon identity,
// the update lane); what is planned is named as planned (§82: unavailable,
// never pretend).

import { useEffect, useState } from "react";
import { useConnection } from "../../state/connection";
import { activeProfile, useConnections } from "../../state/connections";
import { useFeedback } from "../../state/feedback";
import { updatesSentence, useUpdates } from "../../state/updates";
import { useUi } from "../../state/ui";
import { discoveryRoots, setDiscoveryRoots } from "../../state/actions";
import { describeError } from "../../state/errors";
import type { DescribedError } from "../../state/errors";
import type { RootsGetResult } from "../../protocol/types";
import { ErrorNote } from "../../ui/ErrorNote";
import { Button } from "../../ui/Button";
import styles from "./SettingsPage.module.css";

const RESERVED: Array<{ name: string; note: string }> = [
  { name: "Dutchmen", note: "The agent side of the panel — its rooms (new tab, chats) are reserved." },
  { name: "Tab groups", note: "Collapsible groups in the strip." },
  { name: "Extensions", note: "Typed addons with declared permissions — the inventory is live at zim://extensions/; contributions are reserved." },
  { name: "Publishing", note: "Package and publish a server through provider APIs." },
];

/** The feedback account rows (ADR-0028): the machine-local GitHub token
 *  the feedback page files issues with — masked, proven on demand, never
 *  rendered back in full. Empty means the browser route stays the route. */
function FeedbackAccountSection() {
  const token = useFeedback((s) => s.token);
  const login = useFeedback((s) => s.login);
  const signIn = useFeedback((s) => s.signIn);
  const setToken = useFeedback((s) => s.setToken);
  const checkSignIn = useFeedback((s) => s.checkSignIn);

  const status =
    token === ""
      ? "No token — feedback opens in your own signed-in browser."
      : signIn === "signed-in"
        ? `Signed in as ${login ?? "?"}.`
        : signIn === "checking"
          ? "Checking…"
          : signIn === "invalid"
            ? "Rejected by GitHub — paste a fresh token."
            : "Saved but unverified — it will be proven at first use.";

  return (
    <section className={styles.section} aria-label="Feedback account">
      <h2 className={styles.sectionTitle}>Feedback account</h2>
      <div className={styles.row}>
        <div className={styles.rowMain}>
          <span className={styles.rowName}>GitHub token for feedback</span>
          <span className={styles.rowDetail}>{status}</span>
          <span className={styles.rowDetail}>
            Stays on this machine; sent only to api.github.com. The browser route needs none.
          </span>
        </div>
        <div className={styles.tokenStack}>
          <input
            className={styles.tokenInput}
            type="password"
            value={token}
            placeholder="ghp_… or github_pat_…"
            aria-label="GitHub token for feedback"
            spellCheck={false}
            autoComplete="off"
            onChange={(e) => setToken(e.target.value)}
          />
          {token !== "" ? (
            <Button variant="ghost" onClick={() => void checkSignIn()}>
              Verify
            </Button>
          ) : null}
        </div>
      </div>
    </section>
  );
}

/** The discovery roots (§64, ADR-0027): the folders the daemon's scan
 *  walks beyond its own instances dir — listed, added, removed here,
 *  every change answered by the daemon's config so the page shows the
 *  machine's truth, never the draft's guess (§82). */
function DiscoveryRootsSection() {
  const [roots, setRoots] = useState<string[] | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<DescribedError | null>(null);

  useEffect(() => {
    discoveryRoots()
      .then((result: RootsGetResult) => setRoots(result.roots))
      .catch((cause: unknown) => setError(describeError(cause)));
  }, []);

  const apply = (next: Promise<RootsGetResult>) => {
    setBusy(true);
    setNote(null);
    setError(null);
    next
      .then((result) => {
        setRoots(result.roots);
        setDraft("");
        setBusy(false);
      })
      .catch((cause: unknown) => {
        setError(describeError(cause));
        setBusy(false);
      });
  };

  const add = () => {
    const path = draft.trim();
    if (path === "" || busy) return;
    if (roots?.includes(path)) {
      setNote("That root is already configured.");
      return;
    }
    apply(setDiscoveryRoots([...(roots ?? []), path]));
  };

  const remove = (path: string) => {
    if (busy) return;
    apply(setDiscoveryRoots((roots ?? []).filter((root) => root !== path)));
  };

  return (
    <section className={styles.section} aria-label="Server discovery">
      <h2 className={styles.sectionTitle}>Server discovery</h2>
      {error ? (
        /* §81: the owning view keeps the alert frame — the note renders
           inside this quiet card, not as a naked sentence. */
        <div className={styles.errorRow} role="alert">
          <ErrorNote error={error} />
        </div>
      ) : null}
      {roots === null && !error ? (
        <p className={styles.rowDetail}>Reading the scan roots…</p>
      ) : null}
      {roots !== null ? (
        <>
          <ul className={styles.list}>
            {roots.map((root) => (
              <li key={root} className={styles.row}>
                <div className={styles.rowMain}>
                  <span className={styles.pathText}>{root}</span>
                  <span className={styles.rowDetail}>
                    Scanned when the New tab asks this machine.
                  </span>
                </div>
                <Button
                  variant="ghost"
                  disabled={busy}
                  aria-label={`Remove root ${root}`}
                  onClick={() => remove(root)}
                >
                  Remove
                </Button>
              </li>
            ))}
          </ul>
          <div className={styles.addRow}>
            <input
              className={styles.pathInput}
              value={draft}
              placeholder="/home/you/servers — a folder that holds server folders"
              aria-label="Add a discovery root"
              spellCheck={false}
              autoComplete="off"
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") add();
              }}
            />
            <Button onClick={add} disabled={busy || draft.trim() === ""}>
              Add root
            </Button>
          </div>
          <p className={styles.rowDetail}>
            Empty by default — the daemon always scans its own instances folder. Extra roots
            are folders of servers kept elsewhere; the scan never follows symlinks and skips
            hidden and staging folders.
          </p>
        </>
      ) : null}
      {note ? (
        <p className={styles.rowDetail} role="status">
          {note}
        </p>
      ) : null}
    </section>
  );
}

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
    ? `ZIM v${installedVersion}`
    : "ZIM — development (browser)";

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
            <Button onClick={() => void installNow()}>Download update</Button>
          ) : null}
          {phase.kind === "ready" ? (
            <Button variant="primary" onClick={() => void restart()}>
              Restart to apply
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
          <span className={styles.rowName}>Download updates automatically</span>
          <span className={styles.rowDetail}>
            Fetch an offered update without asking. Applying it is the restart — and the restart
            is always yours to make.
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
            Installers and the update manifest live on this project's
            public{" "}
            <a
              className={styles.channelLink}
              href="https://github.com/ZaminMC/ZIM/releases"
              target="_blank"
              rel="noreferrer"
            >
              ZIM releases
            </a>{" "}
            page, signed before they are offered.
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
          <p className={styles.subtitle}>Settings for this panel. Server settings live with each server.</p>
        </header>

        <section className={styles.section} aria-label="Connections">
          <h2 className={styles.sectionTitle}>Connections</h2>
          <ul className={styles.list}>
            <li className={styles.row}>
              <div className={styles.rowMain}>
                <span className={styles.rowName}>This machine</span>
                <span className={styles.rowDetail}>ZIM on this machine</span>
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

        <section className={styles.section} aria-label="Service">
          <h2 className={styles.sectionTitle}>Service</h2>
          <div className={styles.row}>
            <div className={styles.rowMain}>
              <span className={styles.rowName}>
                {daemon ? `ZIM v${daemon.version}` : "ZIM's service"}
              </span>
              <span className={styles.rowDetail} title={lastError ?? undefined}>
                {statusLabel}
                {lastError ? ` — ${lastError}` : ""}
              </span>
            </div>
          </div>
        </section>

        <DiscoveryRootsSection />

        <UpdatesSection />

        <FeedbackAccountSection />

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
