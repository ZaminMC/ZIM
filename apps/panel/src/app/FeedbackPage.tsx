// zaminpanel://feedback/ — the feedback page (ADR-0028). An operator-typed
// report — title, details, an optional pasted screenshot — sent to the
// development repo as a real GitHub issue, through one of two stated
// routes: the panel's own POST when a token is configured, or the
// operator's own signed-in browser when not. The page names its route, its
// account, and what happens to the screenshot before anything leaves —
// §82's honesty, applied to a lane that talks to a live API.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useConnection } from "../state/connection";
import {
  diagnosticsBlock,
  useFeedback,
  type FeedbackIdentity,
  type SendOutcome,
} from "../state/feedback";
import { useUpdates } from "../state/updates";
import { Button } from "../ui/Button";
import { ErrorNote } from "../ui/ErrorNote";
import { describeError } from "../state/errors";
import styles from "./FeedbackPage.module.css";

/** The OS calls the page's send routes need — injected, so the routing
 *  decisions are testable without a desktop host. */
export interface FeedbackBridge {
  openUrl(url: string): Promise<{ ok: true } | { ok: false; message: string }>;
  copyImage(png: Uint8Array): Promise<{ ok: true } | { ok: false; message: string }>;
}

let bridge: FeedbackBridge | null = null;
/** Test seam: swap the OS-call backend (withEditors-style, per STYLE-GUIDE). */
export function setFeedbackBridgeForTests(next: FeedbackBridge | null): void {
  bridge = next;
}
async function currentBridge(): Promise<FeedbackBridge | null> {
  if (bridge) return bridge;
  const { realFeedbackBackend } = await import("../integration/feedbackBridge");
  return realFeedbackBackend();
}

/** The platform string the diagnostics block carries. Deterministic where
 *  the runtime answers, honest where it does not. */
function platformLabel(): string {
  if (typeof navigator === "undefined") return "unknown";
  const ua = navigator.userAgent;
  if (ua.includes("Windows")) return "Windows";
  if (ua.includes("Mac")) return "macOS";
  if (ua.includes("Linux")) return "Linux";
  return "unknown";
}

/** A pasted screenshot, held as bytes + a preview URL. */
interface PastedShot {
  bytes: Uint8Array;
  previewUrl: string;
}

export function FeedbackPage() {
  const token = useFeedback((s) => s.token);
  const login = useFeedback((s) => s.login);
  const signIn = useFeedback((s) => s.signIn);
  const sending = useFeedback((s) => s.sending);
  const checkSignIn = useFeedback((s) => s.checkSignIn);
  const send = useFeedback((s) => s.send);

  const installedVersion = useUpdates((s) => s.installedVersion);
  const connectionOpen = useConnection((s) => s.status === "ready");

  const [title, setTitle] = useState("");
  const [details, setDetails] = useState("");
  const [shot, setShot] = useState<PastedShot | null>(null);
  const [outcome, setOutcome] = useState<SendOutcome>({ kind: "idle" });
  const [clipboardNote, setClipboardNote] = useState<string | null>(null);
  const [bridgeError, setBridgeError] = useState<string | null>(null);
  const shotRef = useRef<PastedShot | null>(null);

  const identity: FeedbackIdentity = useMemo(
    () => ({ installedVersion, platform: platformLabel() }),
    [installedVersion],
  );

  // The account row proves (or honestly retracts) the sign-in on arrival.
  useEffect(() => {
    if (token !== "" && signIn === "unknown") void checkSignIn();
  }, [token, signIn, checkSignIn]);

  useEffect(
    () => () => {
      // The preview URL is page-local; a closed page releases it.
      if (shotRef.current) URL.revokeObjectURL(shotRef.current.previewUrl);
    },
    [],
  );

  const onPaste = useCallback((event: React.ClipboardEvent) => {
    const items = Array.from(event.clipboardData.items);
    const image = items.find((item) => item.type.startsWith("image/"));
    if (!image) return;
    const file = image.getAsFile();
    if (!file) return;
    void (async () => {
      const buffer = await file.arrayBuffer();
      const bytes = new Uint8Array(buffer);
      if (bytes.byteLength > 8 * 1024 * 1024) {
        setClipboardNote("That image is over 8 MB — GitHub's paste-attach will refuse it. Try a smaller crop.");
        return;
      }
      if (shotRef.current) URL.revokeObjectURL(shotRef.current.previewUrl);
      const previewUrl = URL.createObjectURL(file);
      shotRef.current = { bytes, previewUrl };
      setShot(shotRef.current);
      setClipboardNote(null);
    })();
  }, []);

  const removeShot = useCallback(() => {
    if (shotRef.current) URL.revokeObjectURL(shotRef.current.previewUrl);
    shotRef.current = null;
    setShot(null);
  }, []);

  const attachShotToClipboard = useCallback(async (): Promise<string | null> => {
    if (!shot) return null;
    const backend = await currentBridge();
    if (!backend) {
      return "The screenshot stays on your clipboard (Ctrl+V) in the GitHub form.";
    }
    const copied = await backend.copyImage(shot.bytes);
    if (!copied.ok) {
      setBridgeError(describeError(copied.message).title);
      return null;
    }
    return "The screenshot was copied back to your clipboard — press Ctrl+V on GitHub to attach it.";
  }, [shot]);

  const onSend = useCallback(async () => {
    setOutcome({ kind: "idle" });
    setClipboardNote(null);
    const result = await send({
      title: title.trim(),
      details,
      identity,
      hasScreenshot: shot !== null,
    });
    setOutcome(result);
    if (result.kind === "created") {
      const note = await attachShotToClipboard();
      setClipboardNote(note);
      const backend = await currentBridge();
      if (backend) {
        const opened = await backend.openUrl(result.url);
        if (!opened.ok) setBridgeError(describeError(opened.message).title);
      }
    }
    if (result.kind === "browser") {
      const note = await attachShotToClipboard();
      setClipboardNote(note);
      const backend = await currentBridge();
      if (backend) {
        const opened = await backend.openUrl(result.url);
        if (!opened.ok) setBridgeError(describeError(opened.message).title);
      }
    }
  }, [title, details, identity, shot, send, attachShotToClipboard]);

  const canSend = title.trim().length > 0 && details.trim().length > 0 && !sending;
  const routeNote =
    token === ""
      ? "No GitHub token on this machine — Send opens the report in your own signed-in browser."
      : signIn === "signed-in"
        ? `Signed in as ${login ?? "?"} — Send files the issue directly from here.`
        : signIn === "checking"
          ? "Checking the saved GitHub token…"
          : signIn === "invalid"
            ? "The saved GitHub token was rejected — refresh it in Settings, or send via browser."
            : "A GitHub token is saved but unverified — it will be proven (or honestly refused) at send time.";

  return (
    <div className={styles.page} onPaste={onPaste}>
      <header className={styles.head}>
        <h1 className={styles.title}>Feedback</h1>
        <p className={styles.sub}>
          Tell the builders what broke, what confused you, or what should exist. Reports land as
          public issues in the ZaminPanel repository.
        </p>
      </header>

      <section className={styles.section} aria-label="Account and route">
        <p className={styles.route} data-signin={signIn}>
          {routeNote}
        </p>
        <p className={styles.route}>
          A screenshot rides the clipboard, not the API — GitHub's issue form pastes it in. The
          token, if one is saved, travels only to api.github.com and never into the report.
        </p>
      </section>

      <section className={styles.section} aria-label="Report">
        <label className={styles.label} htmlFor="feedback-title">
          Title
        </label>
        <input
          id="feedback-title"
          className={styles.input}
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="What happened, in one line"
          spellCheck={false}
          maxLength={120}
        />
        <label className={styles.label} htmlFor="feedback-details">
          Details
        </label>
        <textarea
          id="feedback-details"
          className={styles.details}
          value={details}
          onChange={(e) => setDetails(e.target.value)}
          placeholder={
            "What you did, what you expected, what happened instead.\nPaste a screenshot with Ctrl+V to attach it."
          }
          spellCheck={false}
          rows={8}
        />
        {shot ? (
          <figure className={styles.shot}>
            <img src={shot.previewUrl} alt="The pasted screenshot" className={styles.shotImage} />
            <figcaption className={styles.shotCaption}>
              Screenshot attached (pasted) — it rides your clipboard at send time.
              <button type="button" className={styles.shotRemove} onClick={removeShot}>
                Remove
              </button>
            </figcaption>
          </figure>
        ) : null}
        <div className={styles.actions}>
          <Button variant="primary" disabled={!canSend} onClick={() => void onSend()}>
            {sending ? "Sending…" : "Send report"}
          </Button>
          <span className={styles.hint}>
            {title.trim() === "" || details.trim()
              ? ""
              : "A title and details are both needed — an issue nobody can read helps nobody."}
          </span>
        </div>
        {outcome.kind === "error" ? (
          <div className={styles.alert} role="alert">
            <ErrorNote
              error={{
                title: outcome.note,
                remediation: [
                  token === ""
                    ? "The browser route needs your pop-ups allowed for GitHub."
                    : "Your text and diagnostics are kept — pressing Send again re-files them.",
                ],
              }}
            />
          </div>
        ) : null}
        {bridgeError ? (
          <div className={styles.alert} role="alert">
            <ErrorNote error={{ title: bridgeError, remediation: ["The report text stays on this page."] }} />
          </div>
        ) : null}
        {outcome.kind === "created" ? (
          <div className={styles.success} role="status">
            Filed as issue #{outcome.issueNumber}.{" "}
            {outcome.url ? <span className={styles.url}>{outcome.url}</span> : null}
            {clipboardNote ? <span className={styles.clipboard}>{clipboardNote}</span> : null}
          </div>
        ) : null}
        {outcome.kind === "browser" ? (
          <div className={styles.success} role="status">
            The report opened in your browser — press Send on GitHub to file it.
            {clipboardNote ? <span className={styles.clipboard}>{clipboardNote}</span> : null}
          </div>
        ) : null}
      </section>

      <section className={styles.section} aria-label="What the report carries">
        <h2 className={styles.sectionTitle}>What the report carries</h2>
        <p className={styles.body}>
          Your words, plus a small diagnostics block so the builders can reproduce: the installed
          version, the platform, and whether a screenshot rides along. Nothing else — no logs, no
          file contents, no server names. The connection to the daemon is{" "}
          {connectionOpen ? "up" : "down"}; the report never includes its traffic.
        </p>
        <pre className={styles.preview}>
          {diagnosticsBlock(
            identity,
            shot !== null,
          )}
        </pre>
      </section>
    </div>
  );
}

