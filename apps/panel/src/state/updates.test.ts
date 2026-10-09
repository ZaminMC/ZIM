// The update lane's decisions (ADR-0024): every phase transition the
// store can make, against an injected backend — no desktop host, no
// network, no real time. The rules under test are the founder's: no fake
// states (§82), honest sentences on failure (§81), the restart never
// forced, and "automatic" means download — the apply is the restart and
// stays the operator's own click.

import { afterEach, describe, expect, it, vi } from "vitest";
import type { UpdateBackend, UpdateOffer } from "../integration/updater";
import {
  AUTO_CHECK_INTERVAL_MS,
  setUpdateBackend,
  startUpdates,
  stopUpdates,
  updatesSentence,
  useUpdates,
} from "./updates";

const offer: UpdateOffer = { version: "0.1.9", notes: "fixes", pubDate: "2026-10-08T00:00:00Z" };

interface FakeBackend extends UpdateBackend {
  checks: number;
  downloads: number;
  applies: number;
}

function fakeBackend(over: Partial<UpdateBackend> = {}): FakeBackend {
  const backend: FakeBackend = {
    checks: 0,
    downloads: 0,
    applies: 0,
    currentVersion: async () => ({ ok: true, value: "0.1.4" }),
    check: async () => {
      backend.checks += 1;
      return { ok: true, value: null };
    },
    download: async () => {
      backend.downloads += 1;
      return { ok: true, value: null };
    },
    applyAndRestart: async () => {
      backend.applies += 1;
      return { ok: true, value: null };
    },
  };
  return Object.assign(backend, over);
}

function resetStore(): void {
  useUpdates.setState({
    phase: { kind: "idle" },
    prefs: { autoCheck: true, autoInstall: true },
    installedVersion: null,
  });
}

describe("the update lane store", () => {
  afterEach(() => {
    stopUpdates();
    setUpdateBackend(null);
    resetStore();
    vi.useRealTimers();
  });

  it("browser dev degrades honestly: manual check → unavailable", async () => {
    setUpdateBackend(null);
    await useUpdates.getState().check(true);
    expect(useUpdates.getState().phase).toEqual({ kind: "unavailable" });
  });

  it("startUpdates with no desktop host answers unavailable and schedules nothing", async () => {
    setUpdateBackend(null);
    vi.useFakeTimers();
    await startUpdates();
    expect(useUpdates.getState().phase).toEqual({ kind: "unavailable" });
    await vi.advanceTimersByTimeAsync(60_000);
    // No backend → no checks were attempted (the lane does not exist here).
    expect(useUpdates.getState().phase).toEqual({ kind: "unavailable" });
  });

  it("an up-to-date answer carries the installed version", async () => {
    const backend = fakeBackend();
    setUpdateBackend(backend);
    useUpdates.setState({ installedVersion: "0.1.4" });
    await useUpdates.getState().check(true);
    expect(useUpdates.getState().phase).toEqual({ kind: "upToDate", version: "0.1.4" });
  });

  it("auto-download runs the fetch lane: offer → download → ready, nothing applies", async () => {
    const backend = fakeBackend();
    backend.check = async () => {
      backend.checks += 1;
      return { ok: true, value: offer };
    };
    setUpdateBackend(backend);
    await useUpdates.getState().check(false);
    expect(backend.checks).toBe(1);
    expect(backend.downloads).toBe(1);
    expect(backend.applies).toBe(0); // the apply is never automatic
    expect(useUpdates.getState().phase).toEqual({ kind: "ready", offer });
  });

  it("auto-download off stops at available; Download update finishes the fetch", async () => {
    const backend = fakeBackend();
    backend.check = async () => {
      backend.checks += 1;
      return { ok: true, value: offer };
    };
    setUpdateBackend(backend);
    useUpdates.setState({ prefs: { autoCheck: true, autoInstall: false } });

    await useUpdates.getState().check(true);
    expect(backend.downloads).toBe(0);
    expect(useUpdates.getState().phase).toEqual({ kind: "available", offer });

    await useUpdates.getState().installNow();
    expect(backend.downloads).toBe(1);
    expect(backend.applies).toBe(0);
    expect(useUpdates.getState().phase).toEqual({ kind: "ready", offer });
  });

  it("a failed check is a human sentence, dismissible back to idle", async () => {
    const backend = fakeBackend({
      check: async () => ({ ok: false, message: "the channel was unreachable: 502" }),
    });
    setUpdateBackend(backend);
    await useUpdates.getState().check(true);
    expect(useUpdates.getState().phase).toEqual({
      kind: "error",
      message: "the channel was unreachable: 502",
    });
    useUpdates.getState().dismiss();
    expect(useUpdates.getState().phase).toEqual({ kind: "idle" });
  });

  it("a failed download is an error carrying the message", async () => {
    const backend = fakeBackend();
    backend.check = async () => ({ ok: true, value: offer });
    backend.download = async () => ({
      ok: false,
      message: "the update download failed: disk full",
    });
    setUpdateBackend(backend);
    await useUpdates.getState().check(false);
    expect(useUpdates.getState().phase).toEqual({
      kind: "error",
      message: "the update download failed: disk full",
    });
  });

  it("a check never stomps a running download", async () => {
    const backend = fakeBackend();
    setUpdateBackend(backend);
    useUpdates.setState({
      phase: { kind: "downloading", offer },
    });
    await useUpdates.getState().check(true);
    expect(backend.checks).toBe(0);
  });

  it("a pending restart is not quietly replaced by a fresh answer", async () => {
    const backend = fakeBackend();
    setUpdateBackend(backend);
    useUpdates.setState({ phase: { kind: "ready", offer } });
    await useUpdates.getState().check(true);
    expect(backend.checks).toBe(0);
    expect(useUpdates.getState().phase).toEqual({ kind: "ready", offer });
  });

  it("a manual check runs even with auto-check off; an automatic one does not", async () => {
    const backend = fakeBackend();
    setUpdateBackend(backend);
    useUpdates.setState({ prefs: { autoCheck: false, autoInstall: true } });
    await useUpdates.getState().check(false);
    expect(backend.checks).toBe(0);
    await useUpdates.getState().check(true);
    expect(backend.checks).toBe(1);
  });

  it("installNow with nothing pending is a quiet no-op", async () => {
    const backend = fakeBackend();
    setUpdateBackend(backend);
    await useUpdates.getState().installNow();
    expect(backend.downloads).toBe(0);
    expect(useUpdates.getState().phase).toEqual({ kind: "idle" });
  });

  it("restart works only from ready, and a refused apply says so", async () => {
    const backend = fakeBackend();
    setUpdateBackend(backend);
    await useUpdates.getState().restart();
    expect(backend.applies).toBe(0);

    useUpdates.setState({ phase: { kind: "ready", offer } });
    await useUpdates.getState().restart();
    expect(backend.applies).toBe(1);

    backend.applyAndRestart = async () => ({ ok: false, message: "the restart failed: denied" });
    await useUpdates.getState().restart();
    expect(useUpdates.getState().phase).toEqual({
      kind: "error",
      message: "the restart failed: denied",
    });
  });

  it("the cadence follows the auto-check preference", async () => {
    const backend = fakeBackend();
    setUpdateBackend(backend);
    vi.useFakeTimers();

    await useUpdates.getState().check(true); // up-to-date → cadence scheduled
    expect(backend.checks).toBe(1);
    await vi.advanceTimersByTimeAsync(AUTO_CHECK_INTERVAL_MS);
    expect(backend.checks).toBe(2);

    useUpdates.getState().setAutoCheck(false);
    await vi.advanceTimersByTimeAsync(AUTO_CHECK_INTERVAL_MS * 2);
    expect(backend.checks).toBe(2);

    useUpdates.getState().setAutoCheck(true); // idle again → one fresh check scheduled
    await vi.advanceTimersByTimeAsync(10_000);
    expect(backend.checks).toBe(3);
  });

  it("rehydrate raises the prefs and drops the runtime phase (§82)", async () => {
    // A previous session that died mid-install must not boot claiming an
    // install happened — only the prefs ride storage.
    localStorage.setItem(
      "zim.updates",
      JSON.stringify({
        state: {
          phase: { kind: "ready", offer },
          installedVersion: "0.1.4",
          prefs: { autoCheck: false, autoInstall: false },
        },
        version: 1,
      }),
    );
    await useUpdates.persist.rehydrate();
    const state = useUpdates.getState();
    expect(state.prefs).toEqual({ autoCheck: false, autoInstall: false });
    expect(state.phase).toEqual({ kind: "idle" });
    localStorage.removeItem("zim.updates");
  });

  it("every phase renders as a human sentence (§81)", () => {
    expect(updatesSentence({ kind: "unavailable" })).toMatch(/desktop build/);
    expect(updatesSentence({ kind: "idle" })).toMatch(/Not checked/);
    expect(updatesSentence({ kind: "checking" })).toMatch(/Asking/);
    expect(updatesSentence({ kind: "upToDate", version: "0.1.4" })).toMatch(/0\.1\.4/);
    expect(updatesSentence({ kind: "available", offer })).toMatch(/0\.1\.9 is available/);
    expect(updatesSentence({ kind: "downloading", offer })).toMatch(/nothing applies until you restart/);
    expect(updatesSentence({ kind: "ready", offer })).toMatch(/restart to apply/);
    expect(updatesSentence({ kind: "error", message: "boom" })).toBe("boom");
  });
});
