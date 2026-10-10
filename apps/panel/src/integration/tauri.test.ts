// The Tauri seam's contract, both lanes. In a plain browser (what the
// dev loop and these tests default to) every helper degrades to null —
// nothing throws, nothing silently pretends to work. Under a stubbed
// `__TAURI_INTERNALS__` the same helpers ride the real API: invoke
// forwards, failures are the API's own (throw-through for invokeHost,
// null for invokeIfTauri), and listenHost unwraps tauri's event
// envelope so handlers take the payload itself.
//
// `isTauri` is a module-load const, so each lane imports a fresh module
// instance (vi.resetModules + dynamic import) with the flag set first.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
const listenMock = vi.fn();
const webviewListenMock = vi.fn();
const openUrlMock = vi.fn();
const writeImageMock = vi.fn();
const fromBytesMock = vi.fn();
const unlisten = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/webview", () => ({
  // The seam's SCOPING LAW: the subscription rides the current webview's
  // target — the mock stands in for `getCurrentWebview().listen`.
  getCurrentWebview: () => ({ listen: webviewListenMock }),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeImage: writeImageMock,
}));
vi.mock("@tauri-apps/api/image", () => ({
  Image: { fromBytes: fromBytesMock },
}));

function setTauriFlag(on: boolean): void {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

beforeEach(() => {
  vi.resetModules();
  invokeMock.mockReset();
  listenMock.mockReset();
  webviewListenMock.mockReset();
  openUrlMock.mockReset();
  writeImageMock.mockReset();
  fromBytesMock.mockReset();
  unlisten.mockReset();
});

afterEach(() => setTauriFlag(false));

describe("the browser lane (no __TAURI_INTERNALS__)", () => {
  it("isTauri is false", async () => {
    const { isTauri } = await import("./tauri");
    expect(isTauri).toBe(false);
  });

  it("invokeIfTauri answers null and never reaches the desktop", async () => {
    const { invokeIfTauri } = await import("./tauri");
    await expect(
      invokeIfTauri("server.start", { id: "a" }),
    ).resolves.toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("the real feedback backend is null — call sites decide what unavailable looks like", async () => {
    const { realFeedbackBackend } = await import("./feedbackBridge");
    await expect(realFeedbackBackend()).resolves.toBeNull();
    expect(openUrlMock).not.toHaveBeenCalled();
  });
});

describe("the desktop lane (__TAURI_INTERNALS__ stubbed)", () => {
  it("isTauri is true", async () => {
    setTauriFlag(true);
    const { isTauri } = await import("./tauri");
    expect(isTauri).toBe(true);
  });

  it("invokeIfTauri forwards the command and args and answers the value", async () => {
    setTauriFlag(true);
    invokeMock.mockResolvedValue({ version: "0.4.34" });
    const { invokeIfTauri } = await import("./tauri");
    await expect(
      invokeIfTauri<{ version: string }>("daemon.identity"),
    ).resolves.toEqual({
      version: "0.4.34",
    });
    expect(invokeMock).toHaveBeenCalledWith("daemon.identity", undefined);
  });

  it("a failing integration command answers null — never takes the caller down", async () => {
    setTauriFlag(true);
    invokeMock.mockRejectedValue(new Error("the host said no"));
    const { invokeIfTauri } = await import("./tauri");
    await expect(invokeIfTauri("host.command")).resolves.toBeNull();
  });

  it("invokeHost forwards and throws through — the caller owns its failure story", async () => {
    setTauriFlag(true);
    invokeMock.mockResolvedValue(42);
    const { invokeHost } = await import("./tauri");
    await expect(invokeHost<number>("the.answer")).resolves.toBe(42);
    invokeMock.mockRejectedValue(new Error("boom"));
    await expect(invokeHost("the.answer")).rejects.toThrow("boom");
  });

  it("listenHost unwraps the event envelope on the WEBVIEW-SCOPED lane", async () => {
    setTauriFlag(true);
    webviewListenMock.mockResolvedValue(unlisten);
    const { listenHost } = await import("./tauri");
    const seen: unknown[] = [];
    const off = await listenHost<{ state: string }>("server://state", (p) =>
      seen.push(p),
    );
    // The subscription rode the current webview's listen — the global
    // pool would hear every webview's events (the cross-tab leak).
    expect(webviewListenMock).toHaveBeenCalledTimes(1);
    expect(listenMock).not.toHaveBeenCalled();
    const registered = webviewListenMock.mock.calls[0]!;
    expect(registered[0]).toBe("server://state");
    const handler = registered[1] as (e: { payload: unknown }) => void;
    handler({
      event: "server://state",
      id: 1,
      payload: { state: "running" },
    } as unknown as {
      payload: unknown;
    });
    expect(seen).toEqual([{ state: "running" }]);
    expect(off).toBe(unlisten);
    off();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("the api seam loads once — the second call reuses the memoized import", async () => {
    setTauriFlag(true);
    webviewListenMock.mockResolvedValue(unlisten);
    const { listenHost } = await import("./tauri");
    await listenHost("a", () => {});
    await listenHost("b", () => {});
    expect(webviewListenMock).toHaveBeenCalledTimes(2);
    expect(webviewListenMock.mock.calls.map((c) => c[0] as string)).toEqual([
      "a",
      "b",
    ]);
  });
});

describe("the real feedback backend (desktop lane)", () => {
  it("openUrl rides the opener plugin and answers ok", async () => {
    setTauriFlag(true);
    openUrlMock.mockResolvedValue(undefined);
    const { realFeedbackBackend } = await import("./feedbackBridge");
    const backend = await realFeedbackBackend();
    expect(backend).not.toBeNull();
    await expect(
      backend!.openUrl("https://github.com/ZaminMC/ZIM/issues"),
    ).resolves.toEqual({
      ok: true,
      value: null,
    });
    expect(openUrlMock).toHaveBeenCalledWith(
      "https://github.com/ZaminMC/ZIM/issues",
    );
  });

  it("a refused open is a visible Result failure, never a throw", async () => {
    setTauriFlag(true);
    openUrlMock.mockRejectedValue(new Error("no handler"));
    const { realFeedbackBackend } = await import("./feedbackBridge");
    const backend = await realFeedbackBackend();
    const result = await backend!.openUrl("https://example.org");
    expect(result.ok).toBe(false);
    if (!result.ok)
      expect(result.message).toContain("the browser did not open: no handler");
  });

  it("copyImage converts the PNG through the Image seam and hands it to the clipboard", async () => {
    setTauriFlag(true);
    fromBytesMock.mockResolvedValue({ kind: "tauri-image" });
    writeImageMock.mockResolvedValue(undefined);
    const { realFeedbackBackend } = await import("./feedbackBridge");
    const backend = await realFeedbackBackend();
    const png = new Uint8Array([137, 80, 78, 71]);
    await expect(backend!.copyImage(png)).resolves.toEqual({
      ok: true,
      value: null,
    });
    expect(fromBytesMock).toHaveBeenCalledWith(png);
    expect(writeImageMock).toHaveBeenCalledWith({ kind: "tauri-image" });
  });

  it("a refused copy names the screenshot in its failure", async () => {
    setTauriFlag(true);
    fromBytesMock.mockRejectedValue("junk bytes");
    const { realFeedbackBackend } = await import("./feedbackBridge");
    const backend = await realFeedbackBackend();
    const result = await backend!.copyImage(new Uint8Array([1]));
    expect(result.ok).toBe(false);
    if (!result.ok)
      expect(result.message).toContain(
        "the screenshot could not be copied: junk bytes",
      );
  });
});
