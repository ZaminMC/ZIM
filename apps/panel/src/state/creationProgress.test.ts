// The creation progress derivation: every number the modal shows comes
// from the daemon's own job events — stages from the machine-readable
// `stage`, bytes from the download subsystem, speed/ETA from measured
// deltas. These tests pin the laws: no fabricated fractions, no
// timer-driven motion, honest unknowns.

import { describe, expect, it } from "vitest";
import {
  artifactFromMessage,
  crackStage,
  formatEta,
  formatSpeed,
  measuredSpeed,
  progressView,
  pushSample,
  stageChecklist,
  terminalOutcome,
} from "./creationProgress";
import type { Job, JobProgress } from "../protocol/types";

describe("artifactFromMessage", () => {
  it("takes the daemon's own noun out of its download sentence", () => {
    expect(artifactFromMessage("downloading paper-1.21.11-34.jar")).toBe(
      "paper-1.21.11-34.jar",
    );
    expect(artifactFromMessage("downloading  fabric-server.jar ")).toBe(
      "fabric-server.jar",
    );
  });

  it("is null for anything that is not a download sentence", () => {
    expect(artifactFromMessage("writing server files")).toBeNull();
    expect(artifactFromMessage(undefined)).toBeNull();
    expect(artifactFromMessage("")).toBeNull();
  });
});

describe("stageChecklist", () => {
  it("marks everything before the stage done, itself active, the rest pending", () => {
    const checklist = stageChecklist("downloading");
    const byId = Object.fromEntries(checklist.map((s) => [s.id, s.status]));
    expect(byId["preparing"]).toBe("done");
    expect(byId["files"]).toBe("done");
    expect(byId["eula"]).toBe("done");
    expect(byId["downloading"]).toBe("active");
    expect(byId["verifying"]).toBe("pending");
    expect(byId["config"]).toBe("pending");
    expect(byId["registering"]).toBe("pending");
  });

  it("keeps the eula step conditional-looking but never blocking", () => {
    // The daemon skips the eula stage when auto-accept is off; the
    // checklist still orders correctly around it.
    const checklist = stageChecklist("verifying");
    expect(checklist.find((s) => s.id === "eula")?.status).toBe("done");
  });
});

describe("measuredSpeed", () => {
  it("computes the slope of the real samples", () => {
    const samples = [
      { atMs: 0, bytes: 0 },
      { atMs: 3000, bytes: 3 * 1024 * 1024 },
    ];
    expect(measuredSpeed(samples)).toBeCloseTo(1024 * 1024, 0);
  });

  it("uses only the last three seconds of the window", () => {
    const samples = [
      { atMs: 0, bytes: 0 }, // outside the window
      { atMs: 10_000, bytes: 0 },
      { atMs: 13_000, bytes: 3 * 1024 * 1024 },
    ];
    expect(measuredSpeed(samples)).toBeCloseTo(1024 * 1024, 0);
  });

  it("refuses sub-jitter slopes", () => {
    const samples = [
      { atMs: 0, bytes: 0 },
      { atMs: 100, bytes: 900 * 1024 },
    ];
    expect(measuredSpeed(samples)).toBeNull();
  });

  it("a stall is an honest null, not a frozen number", () => {
    const samples = [
      { atMs: 0, bytes: 5 * 1024 * 1024 },
      { atMs: 3000, bytes: 5 * 1024 * 1024 },
    ];
    expect(measuredSpeed(samples)).toBeNull();
  });

  it("a single sample cannot answer", () => {
    expect(measuredSpeed([{ atMs: 0, bytes: 10 }])).toBeNull();
  });

  it("pushSample keeps the window bounded", () => {
    let samples: ReturnType<typeof pushSample> = [];
    for (let i = 0; i < 40; i += 1) {
      samples = pushSample(samples, i * 1000, i * 1024 * 512);
    }
    // 2x window cap = at most ~7 entries retained.
    expect(samples.length).toBeLessThanOrEqual(7);
    expect(samples.at(-1)?.bytes).toBe(39 * 1024 * 512);
  });
});

describe("formatEta / formatSpeed", () => {
  it("formats minutes:seconds from real inputs", () => {
    expect(formatEta(31 * 1024 * 1024, 1024 * 1024)).toBe("00:31");
    expect(formatEta(61 * 1024 * 1024, 1024 * 1024)).toBe("01:01");
  });

  it("refuses to answer without a speed or with nothing left", () => {
    expect(formatEta(1024, null)).toBeNull();
    expect(formatEta(0, 1024 * 1024)).toBeNull();
    expect(formatEta(1024, 0)).toBeNull();
  });

  it("refuses absurd answers (a day-plus ETA is an unknown)", () => {
    expect(formatEta(100 * 1024 * 1024 * 1024, 1024)).toBeNull();
  });

  it("formats the three magnitudes", () => {
    expect(formatSpeed(512)).toBe("512 B/s");
    expect(formatSpeed(64 * 1024)).toBe("64 KB/s");
    expect(formatSpeed(12.4 * 1024 * 1024)).toBe("12.4 MB/s");
    expect(formatSpeed(null)).toBeNull();
  });
});

describe("progressView", () => {
  it("derives bytes and percent only from byte-unit progress", () => {
    const view = progressView({
      current: 428,
      total: 912,
      unit: "bytes",
      stage: "downloading",
      message: "downloading paper.jar",
    });
    expect(view.stage).toBe("downloading");
    expect(view.bytesDone).toBe(428);
    expect(view.bytesTotal).toBe(912);
    expect(view.percent).toBe(47);
    expect(view.artifact).toBe("paper.jar");
  });

  it("never fabricates a percent without totals", () => {
    const view = progressView({
      current: 5,
      unit: "steps",
      stage: "config",
      message: "writing the server configuration",
    });
    expect(view.percent).toBeNull();
    expect(view.bytesDone).toBeNull();
    expect(view.bytesTotal).toBeNull();
  });

  it("an artifact with no published size reports no percent (held, not faked)", () => {
    const view = progressView({
      current: 5_000_000,
      total: undefined,
      unit: "bytes",
      stage: "downloading",
      message: "downloading fabric-server.jar",
    });
    expect(view.percent).toBeNull();
    expect(view.bytesTotal).toBeNull();
    expect(view.bytesDone).toBe(5_000_000);
  });

  it("a pre-staged job (old daemon) degrades to message-only", () => {
    const view = progressView({ current: 0, message: "working" });
    expect(view.stage).toBeNull();
    expect(view.message).toBe("working");
  });

  it("clamps a lying current above total", () => {
    const view = progressView({ current: 2000, total: 1000, unit: "bytes", stage: "downloading" });
    expect(view.percent).toBe(100);
  });
});

describe("crackStage", () => {
  it("maps the quick phases and walks percent through the long one", () => {
    expect(crackStage(progressView({ current: 0, stage: "preparing" }))).toBe(1);
    expect(crackStage(progressView({ current: 0, stage: "files" }))).toBe(2);
    expect(crackStage(progressView({ current: 0, stage: "eula" }))).toBe(2);
    // Downloading: 3 at zero, 9 at done.
    expect(
      crackStage(progressView({ current: 0, total: 100, unit: "bytes", stage: "downloading" })),
    ).toBe(3);
    expect(
      crackStage(progressView({ current: 50, total: 100, unit: "bytes", stage: "downloading" })),
    ).toBe(6);
    expect(
      crackStage(progressView({ current: 100, total: 100, unit: "bytes", stage: "downloading" })),
    ).toBe(9);
    expect(crackStage(progressView({ current: 0, stage: "verifying" }))).toBe(9);
    expect(crackStage(progressView({ current: 0, stage: "registering" }))).toBe(10);
  });

  it("a size-less download holds its entry crack — never paces a fake ramp", () => {
    expect(crackStage(progressView({ current: 1, unit: "bytes", stage: "downloading" }))).toBe(3);
  });

  it("an unstaged job is an unbroken block", () => {
    expect(crackStage(progressView(undefined))).toBe(0);
  });
});

describe("terminalOutcome", () => {
  const job = (state: Job["state"]): Job =>
    ({
      jobId: "j",
      kind: "server.create",
      state,
      createdAtMs: 0,
    }) as unknown as Job;

  it("reads the three terminal states, null while running", () => {
    expect(terminalOutcome(job("running"))).toBeNull();
    expect(terminalOutcome(job("queued"))).toBeNull();
    expect(terminalOutcome(job("succeeded"))).toBe("succeeded");
    expect(terminalOutcome(job("failed"))).toBe("failed");
    expect(terminalOutcome(job("cancelled"))).toBe("cancelled");
    expect(terminalOutcome(undefined)).toBeNull();
  });
});

describe("the JobProgress wire shape", () => {
  it("the staged dialect parses from the daemon's own event", () => {
    const raw: JobProgress = {
      current: 428,
      total: 912,
      unit: "bytes",
      message: "downloading paper-1.21.11-34.jar",
      stage: "downloading",
    };
    expect(raw.stage).toBe("downloading");
  });
});
