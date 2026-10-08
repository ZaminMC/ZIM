// Jobs store: the reconciled view of long-running daemon jobs (§7).
// Seeded from `job.started` events (the runner publishes every
// transition), updated by progress/completion events, and queried by
// views that show a live progress chip (Backups) instead of guessing
// whether a job is still running.

import { create } from "zustand";
import type { Job, JobProgress } from "../protocol/types";

interface JobsState {
  jobs: Record<string, Job>;
  /** job.started: insert (or refresh) the record. */
  started: (job: Job) => void;
  /** job.progress: merge the progress block. */
  progress: (jobId: string, progress: JobProgress) => void;
  /** job.completed: flip the state and attach the error. */
  completed: (jobId: string, outcome: Job["state"], error?: Job["error"]) => void;
  /** Drop finished jobs for a server whose view is being torn down; the
   *  daemon's own history is authoritative and bounded on its side. */
  forgetFinished: (serverId: string) => void;
  /** §73's jobs page: upsert a `jobs.list` snapshot (the authoritative,
   *  reconnection-proof record). Live events keep flowing on top. */
  seedAll: (jobs: Job[]) => void;
}

export const useJobs = create<JobsState>((set) => ({
  jobs: {},

  started: (job) => set((state) => ({ jobs: { ...state.jobs, [job.jobId]: job } })),

  progress: (jobId, progress) =>
    set((state) => {
      const job = state.jobs[jobId];
      if (!job) return state;
      return { jobs: { ...state.jobs, [jobId]: { ...job, progress } } };
    }),

  completed: (jobId, outcome, error) =>
    set((state) => {
      const job = state.jobs[jobId];
      if (!job) return state;
      return {
        jobs: {
          ...state.jobs,
          [jobId]: {
            ...job,
            state: outcome,
            ...(error === undefined ? {} : { error }),
            endedAtMs: Date.now(),
          },
        },
      };
    }),

  forgetFinished: (serverId) =>
    set((state) => {
      const next = { ...state.jobs };
      for (const [id, job] of Object.entries(next)) {
        if (job.serverId === serverId && job.state !== "running" && job.state !== "queued") {
          delete next[id];
        }
      }
      return { jobs: next };
    }),

  /** §73's jobs page seed: the daemon's `jobs.list` snapshot is the
   *  authoritative record (it survives reconnections and keeps the newest
   *  50 finished jobs), so a page mount upserts every row it answers.
   *  Live events keep flowing on top; nothing is dropped on reseed. */
  seedAll: (jobs) =>
    set((state) => {
      const next = { ...state.jobs };
      for (const job of jobs) next[job.jobId] = job;
      return { jobs: next };
    }),
}));

/** The job currently running for a server of one kind, if any. */
export function runningJob(
  jobs: Record<string, Job>,
  serverId: string,
  kind: Job["kind"],
): Job | undefined {
  return Object.values(jobs).find(
    (job) => job.serverId === serverId && job.kind === kind && job.state === "running",
  );
}
