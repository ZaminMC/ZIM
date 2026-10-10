// One structured log line as a row — the shared presentation of the
// console (§25) and the log viewer, split out of both so the ROW itself
// can be memoized (§30: the console UI is never the bottleneck).
//
// The feed's rows are stable object references across appends — the
// buffer copies the ARRAY, never the lines — so React.memo bails for
// every row a batch did not add. Before this component, each append
// re-diffed the full row tree (and the old index-derived keys remounted
// all of it on every prepend and trim); now an append costs the new
// rows and nothing else, while `content-visibility` keeps the offscreen
// mass cheap in the compositor (ADR-0020's law, unchanged).
//
// The seam marker ("— live —") rides the FIRST live row as a `seam`
// prop, exactly where both views drew it inline: one element, one
// re-render when the seam moves.

import { memo } from "react";
import type { LogLine, LogLevel } from "../protocol/types";

/** The level → CSS-module class name both log stylesheets speak. A
 *  missing class (the console has no levelInfo tint; neither has one
 *  for `unknown`) falls back to the plain level span — the module's
 *  index answers undefined, and the row stays honest. */
const LEVEL_CLASSES: Record<LogLevel, string> = {
  info: "levelInfo",
  warn: "levelWarn",
  error: "levelError",
  debug: "levelDebug",
  unknown: "levelUnknown",
};

/** A CSS module's shape as the row sees it: the named classes plus the
 *  level tints addressed through the module's index. */
export type LogRowStyles = Record<string, string>;

export const LogRow = memo(function LogRow({
  line,
  seam,
  styles,
}: {
  line: LogLine;
  /** True only on the first live row — the seam marker rides it. */
  seam: boolean;
  styles: LogRowStyles;
}) {
  const tint = styles[LEVEL_CLASSES[line.level]] ?? "";
  return (
    <>
      {seam ? <p className={styles.seam}>— live —</p> : null}
      <div className={styles.row}>
        <span className={tint === "" ? styles.level : `${styles.level} ${tint}`}>
          {line.level}
        </span>
        {line.thread ? <span className={styles.thread}>[{line.thread}]</span> : null}
        <span className={styles.message}>{line.line}</span>
      </div>
    </>
  );
});
