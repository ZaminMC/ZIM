// The creation block: a Minecraft-inspired block whose face cracks
// through the classic ten destroy stages as REAL creation state
// advances (§21's visual language, ADR-0030's motion rules). The crack
// stage comes from state/creationProgress's derivation of the daemon's
// own job events — never a timer. On success the block resolves (the
// creation completes); on failure it stops where it stopped, honestly,
// with the error speaking beside it.
//
// The pixel art is inline SVG <rect>s (crisp, tiny, no assets): a
// grass-block face in ZIM's own palette, and one deterministic crack
// path per stage — the same density language Minecraft's destroy
// overlay uses, hand-drawn rather than scraped.

import { useRef } from "react";
import type { CreationProgressView } from "../state/creationProgress";
import { crackStage as deriveCrackStage } from "../state/creationProgress";
import styles from "./CreationBlock.module.css";

/** The block face: 16×16 pixel grid, ZIM palette. */
function BlockFace() {
  // Deterministic pixel noise (a hand-tuned seed pattern, not random):
  // the face reads as stone-adjacent rather than a flat fill.
  const seedNoise: Record<string, string> = {
    "2,3": "#6d8a5a", "5,2": "#79a065", "9,4": "#6d8a5a", "13,3": "#79a065",
    "3,8": "#6d8a5a", "7,7": "#5f7a4f", "11,9": "#6d8a5a", "14,8": "#5f7a4f",
    "4,13": "#6d8a5a", "8,12": "#79a065", "12,14": "#5f7a4f",
  };
  return (
    <svg
      className={styles.face}
      viewBox="0 0 16 16"
      shapeRendering="crispEdges"
      aria-hidden="true"
    >
      <rect width="16" height="16" fill="#7ba05b" />
      <rect y="0" width="16" height="4" fill="#8fbf6f" />
      <rect y="4" width="16" height="1" fill="#6d8a5a" />
      {Object.entries(seedNoise).map(([cell, fill]) => {
        const [x, y] = cell.split(",").map(Number);
        return <rect key={cell} x={x} y={y} width="1" height="1" fill={fill} />;
      })}
    </svg>
  );
}

/**
 * One stage's crack pixels (stage 1–10, growing density). Coordinates
 * are hand-placed on the 16×16 grid so every stage is a superset-ish
 * scattering of the last — the destroy overlay's own visual law.
 */
const CRACK_PIXELS: Record<number, Array<[number, number]>> = {
  1: [[7, 6], [8, 7]],
  2: [[7, 6], [8, 7], [6, 8], [9, 9]],
  3: [[7, 6], [8, 7], [6, 8], [9, 9], [5, 5], [10, 10]],
  4: [[7, 6], [8, 7], [6, 8], [9, 9], [5, 5], [10, 10], [4, 9], [11, 6]],
  5: [
    [7, 6], [8, 7], [6, 8], [9, 9], [5, 5], [10, 10], [4, 9], [11, 6],
    [3, 7], [12, 9],
  ],
  6: [
    [7, 6], [8, 7], [6, 8], [9, 9], [5, 5], [10, 10], [4, 9], [11, 6],
    [3, 7], [12, 9], [6, 11], [9, 3],
  ],
  7: [
    [7, 6], [8, 7], [6, 8], [9, 9], [5, 5], [10, 10], [4, 9], [11, 6],
    [3, 7], [12, 9], [6, 11], [9, 3], [2, 10], [13, 5],
  ],
  8: [
    [7, 6], [8, 7], [6, 8], [9, 9], [5, 5], [10, 10], [4, 9], [11, 6],
    [3, 7], [12, 9], [6, 11], [9, 3], [2, 10], [13, 5], [5, 13], [10, 2],
  ],
  9: [
    [7, 6], [8, 7], [6, 8], [9, 9], [5, 5], [10, 10], [4, 9], [11, 6],
    [3, 7], [12, 9], [6, 11], [9, 3], [2, 10], [13, 5], [5, 13], [10, 2],
    [1, 4], [14, 12],
  ],
  10: [
    [7, 6], [8, 7], [6, 8], [9, 9], [5, 5], [10, 10], [4, 9], [11, 6],
    [3, 7], [12, 9], [6, 11], [9, 3], [2, 10], [13, 5], [5, 13], [10, 2],
    [1, 4], [14, 12], [1, 13], [14, 3], [8, 1], [7, 14], [3, 2], [12, 14],
  ],
};

/** The resolved state: the block "drops" — one compositor-only step,
 *  ADR-0030's entrance vocabulary, disabled under reduced motion. */
export function CreationBlock({
  view,
  succeeded,
  failed,
}: {
  view: CreationProgressView;
  succeeded: boolean;
  failed: boolean;
}) {
  // Cracks never heal: the running max across the job's life. A total
  // arriving late (headers first, size after) must not reset the face.
  // The ref write is render-phase but idempotent and monotonic — the
  // one standard exemption for "max so far" derivation.
  const stage = deriveCrackStage(view);
  const maxRef = useRef(0);
  const crack = Math.max(maxRef.current, stage);
  maxRef.current = crack;
  const pixels = CRACK_PIXELS[crack] ?? [];

  return (
    <div
      className={styles.block}
      data-stage={crack}
      data-succeeded={succeeded || undefined}
      data-failed={failed || undefined}
      role="img"
      aria-label={
        succeeded
          ? "Server created"
          : failed
            ? "Creation failed"
            : `Creating the server — crack stage ${crack} of 10`
      }
    >
      <BlockFace />
      {pixels.length > 0 ? (
        <svg className={styles.cracks} viewBox="0 0 16 16" shapeRendering="crispEdges" aria-hidden="true">
          {pixels.map(([x, y]) => (
            <rect key={`${x}-${y}`} x={x} y={y} width="1" height="1" fill="#1d1f17" opacity="0.78" />
          ))}
        </svg>
      ) : null}
    </div>
  );
}
