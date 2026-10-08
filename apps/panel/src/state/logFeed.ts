// The log feed engine (ADR-0020, corrected P0): one live-joined line feed
// that every structured view plays by the same rules — the Logs workspace
// tab and the console (embedded and the dedicated §27 tab) alike. One
// seam discipline (stripLiveOverlap), one paging cursor, one reconnect
// re-seed, one bounded buffer.
//
// The live source is the daemon's ingested PROCESS OUTPUT (stdout/stderr
// → parse → ring → stream) and never a file. Subscribing without a
// cursor, the daemon hands the ring's tail (last ~500 ingested lines) as
// the opening batch — real output, available before any log file exists.
// The file (logs/latest.log, written by the server) is only the DEEP
// history: paged in on scroll-up, consulted on reconnect, and its
// absence is a stated state (historyAvailable: false), not an error.

import { useCallback, useEffect, useRef, useState } from "react";
import { rangeLogs, subscribeLogs } from "./actions";
import type { LogLine, ProtocolErrorObject } from "../protocol/types";
import { logWarn } from "../logger";

export const PAGE_LINES = 200;

export function asProtocolError(error: unknown): ProtocolErrorObject {
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

export interface LogFeedState {
  /** History pages + live lines, in order. Null until the seed resolves. */
  lines: LogLine[] | null;
  /** Index in `lines` where the live section begins (the seam marker). */
  liveStart: number;
  /** startOffset of the oldest loaded page; 0 = no older history pages. */
  cursor: number;
  /** False when the server has written no log file yet: the console is
   *  live output alone, and the view says so once instead of failing. */
  historyAvailable: boolean;
  loadingOlder: boolean;
  error: ProtocolErrorObject | null;
  /** Live lines waiting while the operator scrolled away from the bottom. */
  newCount: number;
  /** Lines the daemon's bounded queue dropped while the operator was
   *  connected — stated, never silently swallowed. */
  missed: number;
}

export const INITIAL_FEED: LogFeedState = {
  lines: null,
  liveStart: 0,
  cursor: 0,
  historyAvailable: true,
  loadingOlder: false,
  error: null,
  newCount: 0,
  missed: 0,
};

/** The feed every structured log view renders from. Owns the stream, the
 *  seam, paging, and the follow gesture; the view owns only presentation. */
export function useLogFeed(serverId: string) {
  const [state, setState] = useState<LogFeedState>(INITIAL_FEED);

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
            historyAvailable: tail.historyAvailable,
            loadingOlder: false,
            error: null,
            newCount: 0,
            missed: 0,
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

  // Live stream first. Subscribing WITHOUT a cursor, the daemon replays
  // its ingested ring tail as the opening batch — process output the
  // server produced before the client even connected, available whether
  // or not a log file exists. Those lines buffer until the seam opens;
  // then the (optional) file history seeds and the overlap is stripped.
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
          if (isDisposed()) return;
          if (notification.payload.kind === "missed") {
            // The daemon's bounded queue dropped lines while the operator
            // was connected. Stated in the view, never silently swallowed;
            // the file holds what the buffer could not (ADR-0006).
            const missed = notification.payload.missed;
            setState((s) => ({ ...s, missed: s.missed + missed }));
            return;
          }
          if (notification.payload.kind !== "logs") return;
          if (!seamOpen) {
            pending.push(...notification.payload.batch);
            return;
          }
          appendLive(notification.payload.batch);
        },
        onRegistered: () => {
          // A resubscribe (reconnect) replays the ring from the daemon's
          // buffer; the file holds anything older (ADR-0006). Re-seed so
          // the seam re-opens against fresh history.
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
      // No initial cursor: the ring tail arrives as the opening batch
      // (replay of what the daemon already ingested from the process).
    )
      .then(async (subscription) => {
        if (isDisposed()) {
          subscription.dispose();
          return;
        }
        disposeStream = () => subscription.dispose();

        // The seed is the seam: everything buffered before it (ring tail
        // included) merges against the file history — overlap stripped
        // inside; from then on the stream is purely live.
        const history = await seedHistory([...pending]);
        pending.length = 0;
        const residual = stripLiveOverlap(history ?? [], [...pending]);
        pending.length = 0;
        seamOpen = true;
        if (residual.length > 0) appendLive(residual);
      })
      .catch((error: unknown) => {
        logWarn("logfeed", "logs stream failed", error);
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

  return { state, listRef, markBottom, loadOlder, jumpToLive, retrySeed };
}
