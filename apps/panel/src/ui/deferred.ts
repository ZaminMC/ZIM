// The deferred list: long collections paint in slices instead of one
// blocking commit. The first paint carries a fixed window — small enough
// that a 2,000-entry directory listing costs the same interaction latency
// as a 100-item one — and the remainder lands over idle frames, while the
// browser has nothing better to do. "Show all" is the explicit escape
// hatch (one commit, on demand, never automatic). A fresh listing resets
// the window: rows that never painted are invisible anyway, so the reset
// is free — and it happens during render, not in an effect, so the first
// commit of new data already carries only the window.

import { useEffect, useMemo, useRef, useState } from "react";

/** Rows committed together with the listing — the budget the first paint pays. */
export const FIRST_PAINT = 120;
/** Rows added per idle frame while the catch-up runs. */
export const CHUNK = 240;

type IdleHandle = number;

const requestIdle: (callback: () => void) => IdleHandle =
  typeof window.requestIdleCallback === "function"
    ? (callback) => window.requestIdleCallback(() => callback())
    : (callback) => window.setTimeout(callback, 16);

const cancelIdle: (handle: IdleHandle) => void =
  typeof window.cancelIdleCallback === "function"
    ? (handle) => window.cancelIdleCallback(handle)
    : (handle) => window.clearTimeout(handle);

export interface DeferredWindow<T> {
  /** `items.slice(0, shown)` — render these now. */
  visible: T[];
  total: number;
  /** Rows that have not painted yet (0 once the catch-up completes). */
  pending: number;
  done: boolean;
  /** Render the whole collection immediately — one explicit commit. */
  showAll: () => void;
}

/**
 * Chunked rendering for long lists. `items` must be referentially stable
 * between renders unless new data actually arrived (a state-held array
 * qualifies; an inline `.map().filter()` chain does not). When the array
 * identity or `resetKey` changes, the window resets to the first-paint
 * budget; re-renders that merely flip unrelated state (busy flags, errors)
 * keep the grown window.
 */
export function useDeferredWindow<T>(items: T[], resetKey?: unknown): DeferredWindow<T> {
  // Render-phase reset (the React-documented "adjust state when a prop
  // changes" pattern): an effect would commit the full old window first
  // and only then shrink it — exactly the frame we are trying to avoid.
  const [state, setState] = useState(() => ({
    items,
    resetKey,
    shown: Math.min(FIRST_PAINT, items.length),
  }));

  let shown = state.shown;
  if (state.items !== items || state.resetKey !== resetKey) {
    shown = Math.min(FIRST_PAINT, items.length);
    setState({ items, resetKey, shown });
  }

  const total = items.length;
  const visible = useMemo(() => items.slice(0, shown), [items, shown]);

  // The idle pump: each completed slice re-renders, this effect re-runs and
  // schedules the next one — the state change drives the loop, so there is
  // no manual rescheduling to leak. The cleanup cancels the pending slice
  // on every re-render and unmount (which is also what makes "Show all"
  // safe: once shown reaches total, the effect simply stops scheduling).
  // StrictMode's double-mount schedules, cancels, and re-schedules — the
  // window ends up in the same place.
  const totalRef = useRef(total);
  totalRef.current = total;

  useEffect(() => {
    if (shown >= total) return;
    const handle = requestIdle(() => {
      setState((prev) => {
        if (prev.items !== items || prev.shown >= totalRef.current) return prev;
        return { ...prev, shown: Math.min(prev.shown + CHUNK, totalRef.current) };
      });
    });
    return () => cancelIdle(handle);
  }, [shown, total, items]);

  const pending = Math.max(total - shown, 0);

  const showAll = () => {
    setState((prev) => ({ ...prev, shown: totalRef.current }));
  };

  return { visible, total, pending, done: pending === 0, showAll };
}
