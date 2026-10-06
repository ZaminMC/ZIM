// Notifications taxonomy (Phase 7): decisions are pure and matrix-tested;
// delivery is Tauri-only and never allowed to throw.

import { describe, expect, it } from "vitest";
import {
  crashNotification,
  isUnfocused,
  jobNotification,
  shouldNotify,
} from "./notifications";

describe("shouldNotify (operator empathy rule)", () => {
  it("fires when the window is hidden", () => {
    expect(shouldNotify({ hidden: true, blurred: false })).toBe(true);
  });

  it("fires when the window is blurred", () => {
    expect(shouldNotify({ hidden: false, blurred: true })).toBe(true);
  });

  it("stays silent while the operator is looking", () => {
    expect(shouldNotify({ hidden: false, blurred: false })).toBe(false);
  });
});

describe("crashNotification", () => {
  it("names the server and the phase", () => {
    const spec = crashNotification({ displayName: "survival", phase: "runtime" });
    expect(spec.title).toBe("survival crashed");
    expect(spec.body).toContain("while running");
  });

  it("distinguishes startup crashes", () => {
    const spec = crashNotification({ displayName: "s", phase: "startup" });
    expect(spec.body).toContain("during startup");
  });

  it("carries the exit code when the daemon knows it", () => {
    const spec = crashNotification({
      displayName: "s",
      phase: "runtime",
      exitCode: 137,
    });
    expect(spec.body).toContain("exit code 137");
  });

  it("omits the exit code when absent", () => {
    const spec = crashNotification({ displayName: "s", phase: "startup" });
    expect(spec.body).not.toContain("exit code");
  });
});

describe("jobNotification", () => {
  it("reports failures with a remediation pointer", () => {
    const spec = jobNotification({ kind: "backup.restore", outcome: "failed", serverName: "demo" });
    expect(spec.title).toBe("Restore — demo failed");
    expect(spec.body).toContain("Open ZaminPanel");
  });

  it("reports successes plainly", () => {
    const spec = jobNotification({
      kind: "server.create",
      outcome: "succeeded",
      serverName: "new-1",
    });
    expect(spec.title).toBe("Server creation — new-1 finished");
  });

  it("reports cancellations as cancellations, not failures", () => {
    const spec = jobNotification({ kind: "backup.create", outcome: "cancelled", serverName: "d" });
    expect(spec.title).toContain("cancelled");
  });

  it("handles an unknown kind honestly", () => {
    const spec = jobNotification({ kind: "future.kind", outcome: "succeeded" });
    expect(spec.title).toBe("Job finished");
  });
});

describe("isUnfocused (ambient shape)", () => {
  it("returns a complete focus shape in jsdom", () => {
    const focus = isUnfocused();
    expect(typeof focus.hidden).toBe("boolean");
    expect(typeof focus.blurred).toBe("boolean");
  });
});
