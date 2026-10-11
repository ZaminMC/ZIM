// The structured console (founder §25–§30, ADR-0020). The server's output
// as typed rows over the same file-backed feed engine as the log viewer —
// so the founder's console rules are render-time views over an intact
// stream, never a second log system:
//   §26  the command input at the bottom, sent over the ordinary stdin
//        path — with terminal recall: Up/Down walks this server's sent
//        commands (per-server, persisted, cross-tab), Escape restores the
//        draft mid-walk
//   §27  the icon that hands the console a dedicated, full-height tab
//   §28  one copy button whose mode (All errors / All warnings / All) is
//        changed by right-click — a contextual menu, not a modal — and
//        whose tint answers the mode (red / yellow / default)
//   §29  the Show all | Info | Warnings | Errors row; filtering never
//        destroys the underlying stream
//   §30  the engine's bounds: paged history, live buffer, cheap rows —
//        the console UI is never the bottleneck.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { sendStdin } from "../state/actions";
import { useServers } from "../state/servers";
import { recordCommand, useConsoleHistory } from "../state/consoleHistory";
import { rowKey, useLogFeed } from "../state/logFeed";
import type { LogLine, LogLevel } from "../protocol/types";
import { Button } from "../ui/Button";
import { IconCopy, IconOpenInNew } from "../ui/icons";
import { LogRow } from "./LogRow";
import styles from "./ConsoleView.module.css";

type ConsoleFilter = "all" | LogLevel;

/** §29's row, in the founder's own words. Debug stays honest: the level
 *  exists in the model, so the row answers it rather than pretending the
 *  stream has four levels. */
const FILTER_CHIPS: ReadonlyArray<{ value: ConsoleFilter; label: string }> = [
  { value: "all", label: "Show all" },
  { value: "info", label: "Info" },
  { value: "warn", label: "Warnings" },
  { value: "error", label: "Errors" },
  { value: "debug", label: "Debug" },
];

type CopyMode = "errors" | "warnings" | "all";

const COPY_MODES: ReadonlyArray<{ value: CopyMode; label: string }> = [
  { value: "errors", label: "All errors" },
  { value: "warnings", label: "All warnings" },
  { value: "all", label: "All" },
];

const DEFAULT_COPY_MODE: CopyMode = "all";

/** The copied text for one line: `[level] [thread] message` — plain text,
 *  paste-ready. The seam markers are not lines; the caller filters them. */
export function copyTextFor(mode: CopyMode, lines: LogLine[]): string {
  const picked = lines.filter((line) => {
    if (mode === "errors") return line.level === "error";
    if (mode === "warnings") return line.level === "warn";
    return true;
  });
  return picked
    .map((line) =>
      line.thread
        ? `[${line.level}] [${line.thread}] ${line.line}`
        : `[${line.level}] ${line.line}`,
    )
    .join("\n");
}

async function writeClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Clipboard permission or a non-secure context: say so, honestly.
    return false;
  }
}

export function ConsoleView({
  serverId,
  variant = "workspace",
  onOpenInNewTab,
}: {
  serverId: string;
  /** Workspace: embedded under the server page's view switch. Dedicated:
   *  the §27 tab, full height of the tab content. */
  variant?: "workspace" | "dedicated";
  /** §27: rendered only where opening the dedicated tab makes sense —
   *  the dedicated tab itself does not carry the icon. */
  onOpenInNewTab?: () => void;
}) {
  const running = useServers((s) => s.servers[serverId]?.state === "running");
  const { state, listRef, markBottom, loadOlder, jumpToLive, retrySeed } = useLogFeed(serverId);

  const [filter, setFilter] = useState<ConsoleFilter>("all");
  const [copyMode, setCopyMode] = useState<CopyMode>(DEFAULT_COPY_MODE);
  const [copyMenuOpen, setCopyMenuOpen] = useState(false);
  const [copyNote, setCopyNote] = useState<string | null>(null);
  // §26's recall lane: the per-server history the composer walks, the
  // walker's position (null = at the live draft), and the draft itself.
  const commandHistory = useConsoleHistory(serverId);
  const [histIndex, setHistIndex] = useState<number | null>(null);
  const draftRef = useRef("");
  const commandRef = useRef<HTMLInputElement>(null);
  const [command, setCommand] = useState("");
  const [stdinNote, setStdinNote] = useState<string | null>(null);

  // Latest-lines mirror for the copy action (§28 copies the loaded view).
  const linesRef = useRef<LogLine[]>([]);
  linesRef.current = state.lines ?? [];

  // The copy menu closes on any outside click or Escape — a contextual
  // menu, not a modal the operator must dismiss (§28).
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!copyMenuOpen) return;
    const close = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setCopyMenuOpen(false);
      }
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setCopyMenuOpen(false);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [copyMenuOpen]);

  const { lines, liveStart, cursor, loadingOlder, error, newCount, historyAvailable, missed } = state;

  // §29: filtering is a view over the intact buffer — the feed keeps every
  // line, the chips only choose which rows render. The non-seam count rides
  // the same pass so the empty state stops re-filtering 20 000 rows on
  // every render (§30's cheap-rows law).
  const { visible, nonSeamCount } = useMemo(() => {
    if (lines === null) return { visible: null, nonSeamCount: 0 };
    const kept = lines
      .map((line, index) => ({ line, index }))
      .filter(({ line, index }) => {
        if (index === liveStart) return true; // the seam marker row
        if (filter !== "all" && line.level !== filter) return false;
        return true;
      });
    let nonSeamCount = 0;
    for (const { index } of kept) if (index !== liveStart) nonSeamCount += 1;
    return { visible: kept, nonSeamCount };
  }, [lines, filter, liveStart]);

  const doCopy = useCallback(
    (mode: CopyMode) => {
      const rows = linesRef.current; // loaded view; the loader pages more in
      const text = copyTextFor(mode, rows);
      const count = text === "" ? 0 : text.split("\n").length;
      void writeClipboard(text).then((ok) => {
        setCopyNote(
          !ok
            ? "the clipboard refused the copy"
            : count === 0
              ? mode === "errors"
                ? "no error lines in the loaded view"
                : mode === "warnings"
                  ? "no warning lines in the loaded view"
                  : "no lines in the loaded view"
              : `copied ${count} line${count === 1 ? "" : "s"}`,
        );
      });
    },
    [],
  );

  // §26's recall walk, with the terminal's own laws: Up steps into the
  // NEWEST entry (the storage is newest-first) and walks older; Down
  // walks back and — before the newest — returns the draft; Escape
  // restores the draft outright. A recalled entry arrives SELECTED (the
  // omnibox's law): the next keystroke types over it instead of
  // appending.
  const recallCommand = useCallback(
    (event: React.KeyboardEvent<HTMLInputElement>) => {
      if (event.key === "ArrowUp" || event.key === "ArrowDown") {
        if (commandHistory.length === 0) return;
        event.preventDefault();
        if (event.key === "ArrowUp") {
          if (histIndex === null) draftRef.current = command;
          const next =
            histIndex === null ? 0 : Math.min(commandHistory.length - 1, histIndex + 1);
          setHistIndex(next);
          setCommand(commandHistory[next] ?? command);
        } else if (histIndex !== null) {
          const next = histIndex - 1;
          if (next < 0) {
            setHistIndex(null);
            setCommand(draftRef.current);
          } else {
            setHistIndex(next);
            setCommand(commandHistory[next] ?? draftRef.current);
          }
        }
        requestAnimationFrame(() => commandRef.current?.select());
      } else if (event.key === "Escape" && histIndex !== null) {
        event.preventDefault();
        setHistIndex(null);
        setCommand(draftRef.current);
        requestAnimationFrame(() => commandRef.current?.select());
      }
    },
    [command, commandHistory, histIndex],
  );

  const submitCommand = useCallback(
    (event: React.FormEvent) => {
      event.preventDefault();
      const line = command.trim();
      if (line === "" || !running) return;
      setStdinNote(null);
      setCommand("");
      setHistIndex(null);
      recordCommand(serverId, line);
      void sendStdin(serverId, line).catch(() => {
        setStdinNote("stdin rejected: is the server running?");
      });
    },
    [command, running, serverId],
  );

  const copyLabel = COPY_MODES.find((m) => m.value === copyMode)?.label ?? "Copy";

  return (
    <section
      className={`${styles.console} ${variant === "dedicated" ? styles.dedicated : ""}`}
      aria-label="Server console"
    >
      <div className={styles.bar}>
        <span className={styles.barLabel}>console</span>
        <span className={styles.barNote}>
          {running ? "stdin connected" : "server is not running — commands are rejected"}
        </span>
        <div className={styles.barActions}>
          {copyNote ? <span className={styles.copyNote}>{copyNote}</span> : null}
          <div className={styles.copyWrap} ref={menuRef}>
            <button
              type="button"
              className={`${styles.copyButton} ${
                copyMode === "errors"
                  ? styles.copyErrors
                  : copyMode === "warnings"
                    ? styles.copyWarnings
                    : ""
              }`}
              onClick={() => doCopy(copyMode)}
              onContextMenu={(event) => {
                event.preventDefault();
                setCopyMenuOpen(true);
              }}
              title={`Copy ${copyLabel.toLowerCase()} from the loaded view — right-click to change the mode`}
              aria-label={`Copy ${copyLabel.toLowerCase()}`}
            >
              <IconCopy size={13} />
              {copyLabel}
            </button>
            {copyMenuOpen ? (
              <div className={styles.copyMenu} role="menu" aria-label="Copy mode">
                {COPY_MODES.map((mode) => (
                  <button
                    key={mode.value}
                    type="button"
                    role="menuitemradio"
                    aria-checked={copyMode === mode.value}
                    className={styles.copyMenuItem}
                    onClick={() => {
                      setCopyMode(mode.value);
                      setCopyMenuOpen(false);
                    }}
                  >
                    {mode.label}
                    {copyMode === mode.value ? <span className={styles.check}>✓</span> : null}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
          {onOpenInNewTab ? (
            <button
              type="button"
              className={styles.iconButton}
              onClick={onOpenInNewTab}
              title="Open console in new tab"
              aria-label="Open console in new tab"
            >
              <IconOpenInNew size={13} />
            </button>
          ) : null}
        </div>
      </div>

      <div className={styles.chips} role="group" aria-label="Console filter">
        {FILTER_CHIPS.map((chip) => (
          <button
            key={chip.value}
            type="button"
            className={`${styles.chip} ${filter === chip.value ? styles.chipActive : ""}`}
            onClick={() => setFilter(chip.value)}
            aria-pressed={filter === chip.value}
          >
            {chip.label}
          </button>
        ))}
      </div>

      <div className={styles.list} ref={listRef} onScroll={markBottom}>
        {visible === null ? (
          <p className={styles.empty}>Loading the console…</p>
        ) : (
          <>
            {error ? (
              <div className={styles.errorNote} role="alert">
                <p>
                  {error.code ? `${error.code}: ` : ""}
                  {error.message}
                </p>
                <button type="button" className={`${styles.chip} ${styles.chipActive}`} onClick={retrySeed}>
                  Retry
                </button>
              </div>
            ) : null}
            {!error && missed > 0 ? (
              <p className={styles.note} role="status">
                {missed} line{missed === 1 ? "" : "s"} were missed while ZIM's buffer
                overflowed — older lines live in the log file.
              </p>
            ) : null}
            {!error && !historyAvailable ? (
              <p className={styles.note} role="status">
                No saved history yet — this console is the live process output.
              </p>
            ) : null}
            {!error && cursor > 0 ? (
              <button
                type="button"
                className={`${styles.chip} ${styles.loadOlder}`}
                onClick={loadOlder}
                disabled={loadingOlder}
              >
                {loadingOlder ? "Loading…" : "Load older lines"}
              </button>
            ) : null}
            {!error && cursor === 0 && historyAvailable && lines !== null && lines.length > 0 ? (
              <p className={styles.startOfLog}>beginning of the log file</p>
            ) : null}
            {nonSeamCount === 0 && !error ? (
              <p className={styles.empty}>
                {filter !== "all"
                  ? "No lines match the current filter — the stream itself is untouched."
                  : "No console output yet — it appears when the server writes output."}
              </p>
            ) : null}
            {visible.map(({ line, index }) => (
              <LogRow
                key={rowKey(state, index)}
                line={line}
                seam={index === liveStart}
                styles={styles}
              />
            ))}
          </>
        )}
      </div>

      {newCount > 0 ? (
        <button type="button" className={styles.jump} onClick={jumpToLive}>
          {newCount} new line{newCount === 1 ? "" : "s"} — jump to live
        </button>
      ) : null}

      <form className={styles.composer} onSubmit={submitCommand}>
        <input
          ref={commandRef}
          className={styles.command}
          type="text"
          value={command}
          onChange={(event) => setCommand(event.target.value)}
          onKeyDown={recallCommand}
          placeholder="Type a Minecraft command…"
          aria-label="Minecraft command"
          title="Up and Down arrows recall previously sent commands; Escape restores the draft"
          disabled={!running}
        />
        <Button type="submit" variant="primary" disabled={!running || command.trim() === ""}>
          Send
        </Button>
        {stdinNote ? (
          <span className={styles.stdinNote} role="alert">
            {stdinNote}
          </span>
        ) : null}
      </form>
    </section>
  );
}
