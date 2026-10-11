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
//
// §30's cheap-rows law reaches the React layer here: every row carries a
// MINTED identity (keys), so appends, prepends, and cap-trims never
// re-key a surviving row — the old index-derived keys remounted the
// whole list on every prepend and on every trim at the buffer cap. And
// the cap's trim is PAGE-GRANULAR in the history: the byte cursor only
// ever lands on a page boundary it can name exactly (a row-count trim
// cannot advance a byte cursor — the rows' byte sizes are unknown — and
// the stale cursor it left behind made the next loadOlder re-open a GAP:
// the page it fetched skipped the rows the trim had dropped).

import { useCallback, useEffect, useRef, useState } from "react";
import { rangeLogs, subscribeLogs } from "./actions";
import type { LogLine, ProtocolErrorObject } from "../protocol/types";
import { logWarn } from "../logger";

export const PAGE_LINES = 200;

/** Ceiling on the in-memory feed: history pages + live lines combined.
 * The buffer used to grow without bound — every batch copied the whole
 * array and busy servers dragged the webview toward OOM over days.
 * Beyond the cap the OLDEST HISTORY is dropped first (live lines are the
 * operator's "now" and stay); `cursor` goes to 0 when history is fully
 * trimmed so loadOlder simply stops at the floor. */
export const MAX_FEED_LINES = 20_000;

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
 * No match keeps every live line: showing a rare duplicate beats losing a
 * line.
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

/** One loaded history page's footprint in the buffer: where it starts in
 *  the file and how many rows it contributed. The cap's trim walks these
 *  — a page is dropped whole or not at all, so the byte cursor only ever
 *  advances to an offset the feed can name exactly. */
export interface PageSpan {
  /** The page's first row's byte offset in the log file. */
  start: number;
  /** Rows the page holds in the buffer's history section. */
  rows: number;
}

export interface LogFeedState {
  /** History pages + live lines, in order. Null until the seed resolves. */
  lines: LogLine[] | null;
  /** One stable React key per row of `lines` — null exactly when `lines`
   *  is. Minted at ingestion from a monotonic counter (never inside a
   *  state updater — updaters stay pure), so a row keeps its identity
   *  across appends, prepends, and cap-trims. Index-derived keys used to
   *  remount the entire list on every prepend and on every trim at the
   *  cap — and duplicate texts in one batch could collide; minted keys
   *  can neither shift nor collide. */
  keys: string[] | null;
  /** Index in `lines` where the live section begins (the seam marker). */
  liveStart: number;
  /** The byte offset the next older page continues from: the oldest
   *  retained page's first row — always a boundary the buffer actually
   *  holds, because the cap's trim never drops the newest page (the
   *  seam's neighbor) and never empties the layout while history is
   *  retained. 0 = the beginning of the file is loaded (or there is no
   *  file): no older pages exist. */
  cursor: number;
  /** The loaded history's page layout, oldest first. Invariants:
   *  `pages.reduce(rows) === liveStart`; every page has rows > 0; after a
   *  cap-trim, at least one page survives whenever any history was
   *  held. */
  pages: PageSpan[];
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
  keys: null,
  liveStart: 0,
  cursor: 0,
  pages: [],
  historyAvailable: true,
  loadingOlder: false,
  error: null,
  newCount: 0,
  missed: 0,
};

/** The stable identity of the row at `index` — the views' one access
 *  point, so the parallel-array bookkeeping never leaks into JSX. */
export function rowKey(feed: LogFeedState, index: number): string {
  return feed.keys?.[index] ?? `${index}`;
}

/** The feed every structured log view renders from. Owns the stream, the
 * seam, paging, and the follow gesture; the view owns only presentation. */
export function useLogFeed(serverId: string) {
  const [state, setState] = useState<LogFeedState>(INITIAL_FEED);

  // Latest-state mirror for callbacks that must decide without racing
  // React's batched updates (paging guards, live buffers).
  const stateRef = useRef(state);
  stateRef.current = state;

  const listRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);
  const pagingRef = useRef(false);

  // The row-key minter. One counter per feed, never rewound: keys stay
  // unique across re-seeds (a re-seed replaces the list, so reuse would
  // be safe — but uniqueness costs nothing and reads honestly).
  const keySeq = useRef(0);
  const mintKeys = useCallback((count: number): string[] => {
    const minted: string[] = new Array<string>(count);
    for (let i = 0; i < count; i += 1) minted[i] = `r${keySeq.current++}`;
    return minted;
  }, []);

  const markBottom = useCallback(() => {
    const el = listRef.current;
    const atBottom = !el || el.scrollHeight - el.scrollTop - el.clientHeight < 48;
    atBottomRef.current = atBottom;
    if (atBottom && stateRef.current.newCount !== 0) {
      setState((s) => ({ ...s, newCount: 0 }));
    }
  }, []);

  const appendLive = useCallback(
    (batch: LogLine[]) => {
      if (batch.length === 0) return;
      // Keys are minted OUTSIDE the updater: state updaters must stay
      // pure (StrictMode double-invokes them to prove it).
      const minted = mintKeys(batch.length);
      setState((s) => {
        if (s.lines === null || s.keys === null) return s; // pre-seam lines buffer in `pending`
        const newCount = atBottomRef.current ? 0 : s.newCount + batch.length;
        let lines = [...s.lines, ...batch];
        let keys = [...s.keys, ...minted];
        let liveStart = s.liveStart;
        let cursor = s.cursor;
        let pages = s.pages;
        if (lines.length > MAX_FEED_LINES) {
          // The cap's trim, in two laws:
          //
          // 1. History falls in WHOLE PAGES off the oldest end, and the
          //    NEWEST page always survives. That survivor is the one page
          //    whose start the next loadOlder can continue from — it abuts
          //    the live seam, so `cursor` stays a boundary the buffer
          //    actually holds. Emptying the pages (the old trim) stranded
          //    the cursor at the newest DROPPED page's start: the next
          //    loadOlder fetched that page's older sibling and spliced it
          //    straight onto the live seam — silently skipping the dropped
          //    page's rows, a gap in exactly the history the operator was
          //    paging through.
          //
          // 2. Once the droppable history is exhausted, the LIVE head falls
          //    by the remainder (a console pinned to the tail keeps its
          //    exact view either way — atBottom scroll is anchored to the
          //    bottom, not an offset). The cap is then enforced exactly.
          //    A page-granular trim may otherwise overshoot the cap by up
          //    to one page — bounded is bounded.
          const excess = lines.length - MAX_FEED_LINES;
          const droppable = Math.max(0, pages.length - 1);
          let cut = 0;
          let dropped = 0;
          while (cut < droppable && dropped < excess) {
            dropped += pages[cut]?.rows ?? 0;
            cut += 1;
          }
          if (cut > 0) pages = pages.slice(cut);
          const liveExcess = Math.max(0, excess - dropped);
          const historyEnd = liveStart;
          lines = [...lines.slice(dropped, historyEnd), ...lines.slice(historyEnd + liveExcess)];
          keys = [...keys.slice(dropped, historyEnd), ...keys.slice(historyEnd + liveExcess)];
          liveStart = historyEnd - dropped;
          // pages[0] now names the oldest RETAINED row — never the
          // dropped rows the buffer no longer holds.
          cursor = pages[0]?.start ?? s.cursor;
        }
        return { ...s, lines, keys, liveStart, cursor, pages, newCount };
      });
    },
    [mintKeys],
  );

  // History seed: the file tail becomes the pages, the live section starts
  // after it, and the seam overlap is stripped. Reused by the mount effect
  // (with pre-seam live buffered in `pending`) and by the retry button
  // (live lines already in state).
  const seedHistory = useCallback(
    (pending: LogLine[]) => {
      return rangeLogs({ serverId, maxLines: PAGE_LINES })
        .then((tail) => {
          const joined = stripLiveOverlap(tail.lines, pending);
          const minted = mintKeys(tail.lines.length + joined.length);
          setState({
            lines: [...tail.lines, ...joined],
            keys: minted,
            liveStart: tail.lines.length,
            cursor: tail.startOffset,
            pages:
              tail.lines.length > 0
                ? [{ start: tail.startOffset, rows: tail.lines.length }]
                : [],
            historyAvailable: tail.historyAvailable,
            loadingOlder: false,
            error: null,
            newCount: 0,
            missed: 0,
          });
          return tail.lines;
        })
        .catch((error: unknown) => {
          const minted = mintKeys(pending.length);
          setState((s) => ({
            ...s,
            lines: pending,
            keys: minted,
            liveStart: 0,
            cursor: 0,
            pages: [],
            loadingOlder: false,
            error: asProtocolError(error),
          }));
        });
    },
    [serverId, mintKeys],
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
        // inside; from then on the stream is purely live. Lines that
        // arrived WHILE the seed was in flight ride behind it (the
        // reconnect path's order; clearing `pending` BEFORE the residual
        // computation used to silently drop them).
        const buffered = [...pending];
        pending.length = 0;
        const history = await seedHistory(buffered);
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
        const minted = mintKeys(page.lines.length);
        setState((prev) => ({
          ...prev,
          lines: prev.lines === null ? null : [...page.lines, ...prev.lines],
          keys: prev.keys === null ? null : [...minted, ...prev.keys],
          cursor: page.startOffset,
          liveStart: prev.liveStart + page.lines.length,
          pages:
            page.lines.length > 0
              ? [
                  { start: page.startOffset, rows: page.lines.length },
                  ...prev.pages,
                ]
              : prev.pages,
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
  }, [serverId, seedHistory, mintKeys]);

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
