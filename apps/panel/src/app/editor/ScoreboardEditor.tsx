// The scoreboard editor (§34): the founder's two-pane shape — the
// configuration on the left, the live scoreboard preview on the right.
// Every keystroke lands in the SAME composable row model Compose edits
// (one pair rewritten, all other lines byte-stable), and the preview is
// a pure view over those rows: there is no second source of truth that
// could drift from the file.

import { useMemo } from "react";
import { parseFormatting } from "./minecraft";
import type { ScoreboardRow, ScoreboardSpec } from "./scoreboard";
import styles from "./ScoreboardEditor.module.css";

/** One preview line: codes parsed, segments styled the way the client
 *  would style them. */
function PreviewLine({ text }: { text: string }) {
  const segments = useMemo(() => parseFormatting(text), [text]);
  if (text.trim() === "") return <li className={styles.previewRow}>&nbsp;</li>;
  return (
    <li className={styles.previewRow}>
      {segments.map((segment, i) => (
        <span
          key={i}
          style={{
            color: segment.color ?? "#ffffff",
            fontWeight: segment.bold ? 700 : 400,
            fontStyle: segment.italic ? "italic" : "normal",
            textDecoration:
              [segment.underline ? "underline" : "", segment.strike ? "line-through" : ""]
                .filter(Boolean)
                .join(" ") || undefined,
            opacity: segment.obfuscated ? 0.75 : undefined,
          }}
        >
          {segment.text}
        </span>
      ))}
    </li>
  );
}

export function ScoreboardEditor({
  spec,
  onTitle,
  onRows,
}: {
  spec: ScoreboardSpec;
  /** Rewrite the title pair through the row model. */
  onTitle: (title: string) => void;
  /** Rewrite the rows through the row model. */
  onRows: (rows: ScoreboardRow[]) => void;
}) {
  const rows = spec.rows;

  const setValue = (index: number, value: string) => {
    onRows(rows.map((row, i) => (i === index ? { ...row, value } : row)));
  };
  const remove = (index: number) => {
    onRows(rows.filter((_, i) => i !== index));
  };
  const move = (index: number, step: -1 | 1) => {
    const target = index + step;
    if (target < 0 || target >= rows.length) return;
    const from = rows[index];
    const to = rows[target];
    if (!from || !to) return;
    // Values swap; the keys stay positional — the file's key order IS
    // the row order, so a move rewrites exactly the two values.
    onRows(
      rows.map((row, i) => ({
        ...row,
        value: i === index ? to.value : i === target ? from.value : row.value,
      })),
    );
  };
  const add = () => {
    if (rows.length >= 32) return;
    onRows([...rows, { key: null, value: "" }]);
  };

  return (
    <div className={styles.editor} aria-label="Scoreboard editor">
      <div className={styles.config}>
        <div className={styles.field}>
          <label className={styles.label} htmlFor="scoreboard-title">
            Title
          </label>
          <input
            id="scoreboard-title"
            className={styles.input}
            value={spec.title}
            maxLength={40}
            onChange={(event) => onTitle(event.target.value)}
            placeholder="&amp;c&amp;lSurvival"
          />
          <span className={styles.hint}>
            &amp;a–&amp;f colors, &amp;l bold — the preview shows what players see
          </span>
        </div>

        <div className={styles.field}>
          <span className={styles.label}>Lines</span>
          <ol className={styles.rows}>
            {rows.map((row, index) => (
              <li key={row.key ?? `new-${index}`} className={styles.row}>
                <span className={styles.index}>{index + 1}</span>
                <input
                  className={styles.input}
                  value={row.value}
                  maxLength={40}
                  aria-label={`Line ${index + 1}`}
                  onChange={(event) => setValue(index, event.target.value)}
                />
                <button
                  type="button"
                  className={styles.mini}
                  aria-label={`Move line ${index + 1} up`}
                  disabled={index === 0}
                  onClick={() => move(index, -1)}
                >
                  ↑
                </button>
                <button
                  type="button"
                  className={styles.mini}
                  aria-label={`Move line ${index + 1} down`}
                  disabled={index === rows.length - 1}
                  onClick={() => move(index, 1)}
                >
                  ↓
                </button>
                <button
                  type="button"
                  className={styles.mini}
                  aria-label={`Remove line ${index + 1}`}
                  onClick={() => remove(index)}
                >
                  ×
                </button>
              </li>
            ))}
          </ol>
          <button type="button" className={styles.add} onClick={add} disabled={rows.length >= 32}>
            Add line {rows.length >= 32 ? "(max 32)" : ""}
          </button>
        </div>

        <p className={styles.layoutNote}>
          {spec.layout === "single"
            ? "This file keeps its lines in one pair — saving rewrites that pair, everything else stays byte for byte."
            : "This file keeps one pair per line — saving rewrites only the lines you touched."}
        </p>
      </div>

      <div className={styles.previewPane} aria-label="Live scoreboard preview">
        <span className={styles.previewTitle}>Live preview</span>
        <div className={styles.preview}>
          <div className={styles.board}>
            <div className={styles.boardTitle}>
              {spec.title.trim() === "" ? (
                <span>Title</span>
              ) : (
                parseFormatting(spec.title).map((segment, i) => (
                  <span
                    key={i}
                    style={{
                      color: segment.color ?? "#ffffff",
                      fontWeight: segment.bold ? 700 : 400,
                      fontStyle: segment.italic ? "italic" : "normal",
                    }}
                  >
                    {segment.text}
                  </span>
                ))
              )}
            </div>
            <ul className={styles.boardLines}>
              {rows.map((row, index) => (
                <PreviewLine key={row.key ?? `preview-${index}`} text={row.value} />
              ))}
            </ul>
          </div>
        </div>
        <span className={styles.previewNote}>
          Placeholders (%online%, %balance% …) render literally — the plugin fills them in game.
        </span>
      </div>
    </div>
  );
}
