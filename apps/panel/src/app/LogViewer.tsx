// Log viewer: the workspace's structured view of a server's full history —
// file-backed pages (logs.range + byte-offset paging), the live logs stream
// joined at the tail, level filters, and text search. Unlike the console
// (structured rows + stdin), nothing here is ephemeral: the file is the
// truth, and the stream only appends to its end (ADR-0006).
//
// Since ADR-0020 the feed machinery lives in state/logFeed.ts, shared with
// the console so both views play identical rules; this component owns the
// logs-specific presentation (search, level chips, the seam).

import { useMemo, useState } from "react";
import { rowKey, useLogFeed } from "../state/logFeed";
import type { LogLevel } from "../protocol/types";
import { LogRow } from "./LogRow";
import styles from "./LogViewer.module.css";

// The feed engine's re-export keeps the historical test import path stable.
export { stripLiveOverlap } from "../state/logFeed";

type LevelFilter = "all" | LogLevel;

const LEVEL_CHIPS: ReadonlyArray<{ value: LevelFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "info", label: "Info" },
  { value: "warn", label: "Warn" },
  { value: "error", label: "Error" },
  { value: "debug", label: "Debug" },
];

export function LogViewer({ serverId }: { serverId: string }) {
  const [levelFilter, setLevelFilter] = useState<LevelFilter>("all");
  const [search, setSearch] = useState("");

  const { state, listRef, markBottom, loadOlder, jumpToLive, retrySeed } = useLogFeed(serverId);
  const { lines, liveStart, cursor, loadingOlder, error, newCount, historyAvailable, missed } =
    state;

  // The non-seam count rides the same pass as the filter — the empty
  // state stops re-walking every row on each render (§30's law, the same
  // one the console answers).
  const { visible, nonSeamCount } = useMemo(() => {
    if (lines === null) return { visible: null, nonSeamCount: 0 };
    const needle = search.trim().toLowerCase();
    const kept = lines
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
    let nonSeamCount = 0;
    for (const { index } of kept) if (index !== liveStart) nonSeamCount += 1;
    return { visible: kept, nonSeamCount };
  }, [lines, levelFilter, search, liveStart]);

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
            {!error && missed > 0 ? (
              <p className={styles.note} role="status">
                {missed} line{missed === 1 ? "" : "s"} were missed while ZIM's buffer
                overflowed — older lines live in the log file.
              </p>
            ) : null}
            {!error && !historyAvailable ? (
              <p className={styles.note} role="status">
                No saved history yet — this view is the live process output.
              </p>
            ) : null}
            {!error && cursor === 0 && historyAvailable && lines !== null && lines.length > 0 ? (
              <p className={styles.startOfLog}>beginning of the log file</p>
            ) : null}
            {nonSeamCount === 0 && !error ? (
              <p className={styles.empty}>
                {search !== "" || levelFilter !== "all"
                  ? "No lines match the current filters."
                  : "No log lines yet — they appear when the server writes output."}
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
        <button className={styles.jump} onClick={jumpToLive}>
          {newCount} new line{newCount === 1 ? "" : "s"} — jump to live
        </button>
      ) : null}
    </section>
  );
}
