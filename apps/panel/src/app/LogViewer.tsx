// Log viewer: the workspace's structured view of a server's full history —
// file-backed pages (logs.range + byte-offset paging), the live logs stream
// joined at the tail, level filters, and text search. Unlike the console
// (a terminal: capped scrollback, stdin), nothing here is ephemeral: the
// file is the truth, and the stream only appends to its end (ADR-0006).
//
// The seam: the page subscribes live-only (a logs cursor skips the ring
// replay) and reads the file tail separately, so the only overlap is the
// lines ingested while both were starting; `stripLiveOverlap` removes that
// window by matching the buffered live prefix against the history suffix.

import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { rangeLogs, subscribeLogs } from "../state/actions";
import type { LogLine, LogLevel, ProtocolErrorObject } from "../protocol/types";
import { logWarn } from "../logger";
import styles from "./LogViewer.module.css";

const PAGE_LINES = 200;

type LevelFilter = "all" | LogLevel;

const LEVEL_CHIPS: ReadonlyArray<{ value: LevelFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "info", label: "Info" },
  { value: "warn", label: "Warn" },
  { value: "error", label: "Error" },
  { value: "debug", label: "Debug" },
];

function asProtocolError(error: unknown): ProtocolErrorObject {
  if (error instanceof Object && "code" in error) {
    const candidate = error as Partial<ProtocolErrorObject>;
    if (typeof candidate.code === "string") {
      return {
        code: candidate.code,
        message:
          typeof candidate.message === "string"
            ? candidate.message
            : error instanceof Error
              ? error.message
              : "the request failed",
        remediation: [],
      };
    }
  }
  return {
    code: "INTERNAL_ERROR",
    message: error instanceof Error ? error.message : "the request failed",
    remediation: [],
  };
}

/**
 * Drop the live-buffered lines the history already contains. The overlap
 * window is "lines ingested between the subscribe and the tail read", so
 * it is always a PREFIX of the live buffer matching a SUFFIX of history —
 * matched by line text (levels/tsMs legitimately differ across sources).
 * No match keeps every live line: showing a rare duplicate beats losing
 * a line.
 */
export function stripLiveOverlap(history: LogLine[], live: LogLine[]): LogLine[] {
  const max = Math.min(history.length, live.length);
  for (let k = max; k > 0; k -= 1) {
    let matched = true;
    for (let i = 0; i < k; i += 1) {
      const fromHistory = history[history.length - k + i];
      const fromLive = live[i];
      if (fromHistory === undefined || fromLive === undefined || fromHistory.line !== fromLive.line) {
        matched = false;
        break;
      }
    }
    if (matched) return live.slice(k);
  }
  return live;
}

interface ViewerState {
  /** History pages + live lines, in order. Null until the tail resolves. */
  lines: LogLine[] | null;
  /** Index in `lines` where the live section begins (the seam marker). */
  liveStart: number;
  /** startOffset of the oldest loaded page; 0 = the file has no older lines. */
  cursor: number;
  loadingOlder: boolean;
  error: ProtocolErrorObject | null;
  /** Live lines waiting while the operator scrolled away from the bottom. */
  newCount: number;
}

const INITIAL: ViewerState = {
  lines: null,
  liveStart: 0,
  cursor: 0,
  loadingOlder: false,
  error: null,
  newCount: 0,
};

export function LogViewer({ serverId }: { serverId: string }) {
  const [state, setState] = useState<ViewerState>(INITIAL);
  const [levelFilter, setLevelFilter] = useState<LevelFilter>("all");
  const [search, setSearch] = useState("");

  // Latest-state mirror for callbacks that must decide without racing
  // React's batched updates (paging guards, live buffers).
  const stateRef = useRef(state);
  stateRef.current = state;

  const listRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);
  const pagingRef = useRef(false);

  const markBottom = useCallback(() => {
    const el = listRef.current;
    const atBottom = !el || el.scrollHeight - el.scrollTop - el.clientHeight < 48;
    atBottomRef.current = atBottom;
    if (atBottom && stateRef.current.newCount !== 0) {
      setState((s) => ({ ...s, newCount: 0 }));
    }
  }, []);

  const appendLive = useCallback((batch: LogLine[]) => {
    if (batch.length === 0) return;
    setState((s) => {
      if (s.lines === null) return s; // pre-seam lines buffer in `pending`
      const newCount = atBottomRef.current ? 0 : s.newCount + batch.length;
      return { ...s, lines: [...s.lines, ...batch], newCount };
    });
  }, []);

  // History seed: the file tail becomes the pages, the live section starts
  // after it, and the seam overlap is stripped. Reused by the mount effect
  // (with pre-seam live buffered in `pending`) and by the retry button
  // (live lines already in state).
  const seedHistory = useCallback(
    (pending: LogLine[]) => {
      return rangeLogs({ serverId, maxLines: PAGE_LINES })
        .then((tail) => {
          const joined = stripLiveOverlap(tail.lines, pending);
          setState({
            lines: [...tail.lines, ...joined],
            liveStart: tail.lines.length,
            cursor: tail.startOffset,
            loadingOlder: false,
            error: null,
            newCount: 0,
          });
          return tail.lines;
        })
        .catch((error: unknown) => {
          setState((s) => ({
            ...s,
            lines: pending,
            liveStart: 0,
            cursor: 0,
            loadingOlder: false,
            error: asProtocolError(error),
          }));
        });
    },
    [serverId],
  );

  // Live stream first (live-only cursor: no ring replay to duplicate the
  // pages), buffered until the seam opens; then the file tail seeds.
  useEffect(() => {
    let disposed = false;
    let disposeStream: (() => void) | null = null;
    const isDisposed = () => disposed;

    const pending: LogLine[] = [];
    let seamOpen = false;

    subscribeLogs(
      serverId,
      {
        onPayload: (notification) => {
          if (isDisposed() || notification.payload.kind !== "logs") return;
          if (!seamOpen) {
            pending.push(...notification.payload.batch);
            return;
          }
          appendLive(notification.payload.batch);
        },
        onRegistered: () => {
          // A resubscribe (reconnect) may have missed lines while the
          // transport was down; the file holds them (ADR-0006), so the
          // history re-seeds from the tail and the seam re-opens. The
          // first registration is the mount flow, handled below.
          if (!seamOpen || isDisposed()) return;
          seamOpen = false;
          const live = stateRef.current.lines?.slice(stateRef.current.liveStart) ?? [];
          void seedHistory(live)
            .then((history) => {
              // Lines ingested while the history re-seeded ride behind it.
              const residual = stripLiveOverlap(history ?? [], [...pending]);
              pending.length = 0;
              if (residual.length > 0) appendLive(residual);
            })
            .finally(() => {
              seamOpen = true;
            });
        },
      },
      // Live-only: a logs cursor skips the daemon's ring replay, which
      // would duplicate the file-backed pages.
      { file: "latest.log", offset: 0 },
    )
      .then(async (subscription) => {
        if (isDisposed()) {
          subscription.dispose();
          return;
        }
        disposeStream = () => subscription.dispose();

        // The tail read is the seam: everything buffered before it seeds
        // the history (overlap stripped inside); anything that arrived
        // during the read is deduped against the fresh history and
        // appended; from then on the stream is purely live.
        const history = await seedHistory([...pending]);
        pending.length = 0;
        const residual = stripLiveOverlap(history ?? [], [...pending]);
        pending.length = 0;
        seamOpen = true;
        if (residual.length > 0) appendLive(residual);
      })
      .catch((error: unknown) => {
        logWarn("logviewer", "logs stream failed", error);
      });

    return () => {
      disposed = true;
      disposeStream?.();
    };
  }, [serverId, appendLive, seedHistory]);

  const loadOlder = useCallback(() => {
    if (pagingRef.current) return;
    const snapshot = stateRef.current;
    if (snapshot.lines === null || snapshot.cursor === 0 || snapshot.error !== null) return;
    pagingRef.current = true;
    setState((s) => ({ ...s, loadingOlder: true }));
    const heightBefore = listRef.current?.scrollHeight ?? 0;

    rangeLogs({ serverId, maxLines: PAGE_LINES, beforeOffset: snapshot.cursor })
      .then((page) => {
        setState((prev) => ({
          ...prev,
          lines: prev.lines === null ? null : [...page.lines, ...prev.lines],
          cursor: page.startOffset,
          liveStart: prev.liveStart + page.lines.length,
          loadingOlder: false,
        }));
        // Keep the operator's viewport anchored on the same rows.
        requestAnimationFrame(() => {
          const node = listRef.current;
          if (node) node.scrollTop += node.scrollHeight - heightBefore;
        });
      })
      .catch((error: unknown) => {
        const described = asProtocolError(error);
        if (described.code === "LOG_CURSOR_INVALID") {
          // The file rotated or truncated under the cursor: history
          // restarts from the tail; live lines stay where they are.
          void seedHistory(stateRef.current.lines?.slice(stateRef.current.liveStart) ?? []);
          return;
        }
        setState((prev) => ({ ...prev, loadingOlder: false, error: described }));
      })
      .finally(() => {
        pagingRef.current = false;
      });
  }, [serverId, seedHistory]);

  const { lines, liveStart, cursor, loadingOlder, error, newCount } = state;

  const visible = useMemo(() => {
    if (lines === null) return null;
    const needle = search.trim().toLowerCase();
    return lines
      .map((line, index) => ({ line, index }))
      .filter(({ line, index }) => {
        if (index === liveStart) return true; // the seam marker row
        if (levelFilter !== "all" && line.level !== levelFilter) return false;
        if (needle === "") return true;
        const haystack = line.thread
          ? `${line.thread} ${line.line}`.toLowerCase()
          : line.line.toLowerCase();
        return haystack.includes(needle);
      });
  }, [lines, levelFilter, search, liveStart]);

  const jumpToLive = useCallback(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
    atBottomRef.current = true;
    setState((s) => ({ ...s, newCount: 0 }));
  }, []);

  const retrySeed = useCallback(() => {
    const live = stateRef.current.lines?.slice(stateRef.current.liveStart) ?? [];
    void seedHistory(live);
  }, [seedHistory]);

  return (
    <section className={styles.viewer} aria-label="Server logs">
      <div className={styles.toolbar}>
        <div className={styles.chips} role="group" aria-label="Level filter">
          {LEVEL_CHIPS.map((chip) => (
            <button
              key={chip.value}
              className={`${styles.chip} ${levelFilter === chip.value ? styles.chipActive : ""}`}
              onClick={() => setLevelFilter(chip.value)}
              aria-pressed={levelFilter === chip.value}
            >
              {chip.label}
            </button>
          ))}
        </div>
        <input
          className={styles.search}
          type="search"
          placeholder="Search lines…"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          aria-label="Search log lines"
        />
      </div>

      <div className={styles.list} ref={listRef} onScroll={markBottom}>
        {visible === null ? (
          <p className={styles.empty}>Loading the log file…</p>
        ) : (
          <>
            {error ? (
              <div className={styles.errorNote} role="alert">
                <p>
                  {error.code ? `${error.code}: ` : ""}
                  {error.message}
                </p>
                <button className={`${styles.chip} ${styles.chipActive}`} onClick={retrySeed}>
                  Retry
                </button>
              </div>
            ) : null}
            {!error && cursor > 0 ? (
              <button
                className={`${styles.chip} ${styles.loadOlder}`}
                onClick={loadOlder}
                disabled={loadingOlder}
              >
                {loadingOlder ? "Loading…" : "Load older lines"}
              </button>
            ) : null}
            {!error && cursor === 0 && lines !== null && lines.length > 0 ? (
              <p className={styles.startOfLog}>beginning of the log file</p>
            ) : null}
            {visible.filter(({ index }) => index !== liveStart).length === 0 && !error ? (
              <p className={styles.empty}>
                {search !== "" || levelFilter !== "all"
                  ? "No lines match the current filters."
                  : "No log lines yet — they appear when the server writes output."}
              </p>
            ) : null}
            {visible.map(({ line, index }) => (
              <Fragment key={`${index}-${line.line}`}>
                {index === liveStart ? <p className={styles.seam}>— live —</p> : null}
                <div className={styles.row}>
                  <span className={`${styles.level} ${styles[`level${capitalize(line.level)}`] ?? ""}`}>
                    {line.level}
                  </span>
                  {line.thread ? <span className={styles.thread}>[{line.thread}]</span> : null}
                  <span className={styles.message}>{line.line}</span>
                </div>
              </Fragment>
            ))}
          </>
        )}
      </div>

      {newCount > 0 ? (
        <button className={styles.jump} onClick={jumpToLive}>
          {newCount} new line{newCount === 1 ? "" : "s"} — jump to live
        </button>
      ) : null}
    </section>
  );
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
