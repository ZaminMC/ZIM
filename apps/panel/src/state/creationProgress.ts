// Creation progress (§7's staged dialect): the New Server modal derives
// EVERYTHING it shows from the daemon's own job events — the stage
// checklist from the machine-readable `stage`, the byte totals from the
// download subsystem's real callbacks, and the speed/ETA from measured
// byte deltas over a rolling window. No timer-driven guesswork: an
// elapsed-time animation that keeps moving when the network stalls is a
// lie, and creation is exactly where a lie hurts most.
//
// This module is pure state derivation — a reducer over the job's
// progress events — so every number it produces is testable without a
// daemon.

import type { Job, JobProgress } from "../protocol/types";

/** The creation lifecycle the daemon's job speaks (engine.rs's own
 *  stages, verbatim ids). One authoritative sequence; the UI never
 *  maintains its own guess at "where we are". */
export type CreationStageId =
  | "preparing"
  | "files"
  | "eula"
  | "downloading"
  | "verifying"
  | "config"
  | "registering";

/** The checklist a stage implies, in order. The eula step is conditional
 *  (only the auto-accept path) and never blocks the sequence. */
const SEQUENCE: CreationStageId[] = [
  "preparing",
  "files",
  "eula",
  "downloading",
  "verifying",
  "config",
  "registering",
];

const STAGE_LABELS: Record<CreationStageId, string> = {
  preparing: "Preparing the directory",
  files: "Writing server files",
  eula: "Accepting the EULA",
  downloading: "Downloading server software",
  verifying: "Verifying the download",
  config: "Writing configuration",
  registering: "Registering the server",
};

export function stageLabel(stage: CreationStageId): string {
  return STAGE_LABELS[stage];
}

/** The artifact name out of a download message ("downloading X" → "X").
 *  The daemon owns the sentence; this is display sugar over its own
 *  noun, never a second source of truth. */
export function artifactFromMessage(message: string | undefined): string | null {
  if (!message) return null;
  const match = /^downloading\s+(.+?)\s*$/.exec(message);
  return match?.[1] ?? null;
}

export type StageStatus = "pending" | "active" | "done";

export interface CreationStageView {
  id: CreationStageId;
  label: string;
  status: StageStatus;
}

/** The checklist for a stage: everything strictly before it is done, it
 *  itself is active, everything after is pending. */
export function stageChecklist(current: CreationStageId): CreationStageView[] {
  const index = SEQUENCE.indexOf(current);
  return SEQUENCE.map((id) => ({
    id,
    label: STAGE_LABELS[id],
    status: index === -1 || SEQUENCE.indexOf(id) < index ? "done" : SEQUENCE.indexOf(id) === index ? "active" : "pending",
  }));
}

// --- speed over a rolling window ----------------------------------------
//
// Bytes arrive as the download subsystem reports them; speed is the
// slope of the last ~3 seconds of REAL samples. A stall zeroes it (the
// ETA disappears rather than freezing at a stale number — an honest
// "unknown" beats a confident wrong answer).

export interface SpeedSample {
  atMs: number;
  bytes: number;
}

const SPEED_WINDOW_MS = 3000;
/** Below this delta the speed is noise, not signal. */
const MIN_SPEED_BPS = 1024;

/** The measured speed over the window, in bytes/second, or null when
 *  the samples cannot honestly answer (too little time, no movement). */
export function measuredSpeed(samples: readonly SpeedSample[]): number | null {
  if (samples.length < 2) return null;
  const newest = samples[samples.length - 1];
  if (newest === undefined) return null;
  const oldestKept =
    samples.find((s) => newest.atMs - s.atMs <= SPEED_WINDOW_MS) ?? samples[0];
  if (oldestKept === undefined || newest === oldestKept) return null;
  const elapsedSec = (newest.atMs - oldestKept.atMs) / 1000;
  if (elapsedSec <= 0.15) return null; // sub-150 ms slopes are jitter
  const speed = (newest.bytes - oldestKept.bytes) / elapsedSec;
  if (!Number.isFinite(speed) || speed < MIN_SPEED_BPS) return null;
  return speed;
}

/** Push a sample into the window (bounded: the caller may pass the
 *  result as its next samples array). */
export function pushSample(
  samples: readonly SpeedSample[],
  atMs: number,
  bytes: number,
): SpeedSample[] {
  const next = [...samples, { atMs, bytes }];
  const newest = next[next.length - 1];
  if (newest === undefined) return next;
  return next.filter((s) => newest.atMs - s.atMs <= SPEED_WINDOW_MS * 2);
}

/** "00:31" from seconds; null when the inputs cannot answer. */
export function formatEta(bytesRemaining: number, speedBps: number | null): string | null {
  if (speedBps === null || speedBps <= 0 || bytesRemaining <= 0) return null;
  const totalSec = Math.round(bytesRemaining / speedBps);
  if (totalSec > 24 * 60 * 60) return null; // absurd → honest unknown
  const minutes = Math.floor(totalSec / 60);
  const seconds = totalSec % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

export function formatSpeed(speedBps: number | null): string | null {
  if (speedBps === null) return null;
  if (speedBps >= 1024 * 1024) return `${(speedBps / (1024 * 1024)).toFixed(1)} MB/s`;
  if (speedBps >= 1024) return `${(speedBps / 1024).toFixed(0)} KB/s`;
  return `${Math.round(speedBps)} B/s`;
}

// --- the derived view the modal renders ----------------------------------

export interface CreationProgressView {
  /** The machine-readable stage the daemon last spoke (null before the
   *  first staged event — the job exists but has not reported). */
  stage: CreationStageId | null;
  /** The human sentence straight off the wire (the daemon's own words). */
  message: string | null;
  /** The download artifact's file name, when the message names one. */
  artifact: string | null;
  /** Bytes done / total, when the current stage reports bytes. */
  bytesDone: number | null;
  bytesTotal: number | null;
  /** Percent 0–100 from the REAL byte counts; null when the stage has
   *  no totals (never a fabricated fraction). */
  percent: number | null;
}

export function progressView(progress: JobProgress | undefined): CreationProgressView {
  const bytes = progress?.unit === "bytes" ? progress : null;
  const percent =
    bytes && bytes.total != null && bytes.total > 0
      ? Math.min(100, Math.round((bytes.current / bytes.total) * 100))
      : null;
  return {
    stage: (progress?.stage as CreationStageId | undefined) ?? null,
    message: progress?.message ?? null,
    artifact: artifactFromMessage(progress?.message),
    bytesDone: bytes ? bytes.current : null,
    bytesTotal: bytes?.total ?? null,
    percent,
  };
}

/** The overall crack stage (0–10) for the block-breaking visual, mapped
 *  from REAL state: byte percent drives the long downloading phase (the
 *  block visibly breaks as verified bytes land), the walked checklist
 *  covers the quick phases around it. Never invents motion — a stall
 *  holds its stage (the honest "still working on this"). The component
 *  keeps the running MAX so cracks never heal when totals appear late. */
export function crackStage(view: CreationProgressView): number {
  switch (view.stage) {
    case null:
      return 0;
    case "preparing":
      return 1;
    case "files":
      return 2;
    case "eula":
      return 2;
    case "downloading": {
      // No byte totals (an artifact that publishes no size): hold the
      // entry crack rather than pacing a fake ramp.
      if (view.percent === null) return 3;
      // 3–9 across the real percent.
      return Math.min(9, 3 + Math.floor((view.percent / 100) * 7));
    }
    case "verifying":
      return 9;
    case "config":
      return 9;
    case "registering":
      return 10;
  }
}

/** A creation job's terminal read: succeeded/failed/cancelled, or null
 *  while it runs. Convenience for view code that already holds the Job. */
export function terminalOutcome(job: Job | undefined): "succeeded" | "failed" | "cancelled" | null {
  if (!job) return null;
  if (job.state === "succeeded" || job.state === "failed" || job.state === "cancelled") {
    return job.state;
  }
  return null;
}
