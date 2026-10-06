// Autostart seam: honest degradation outside the desktop host, faithful
// mirroring inside it. `isTauri` is read at module evaluation, so each test
// resets the module registry and controls `window.__TAURI_INTERNALS__`
// before importing.

import { beforeEach, describe, expect, it, vi } from "vitest";

type Invoke = (command: string, args?: unknown) => Promise<unknown>;

async function importSeam(invoke?: Invoke) {
  vi.resetModules();
  if (invoke) {
    vi.doMock("@tauri-apps/api/core", () => ({ invoke }));
  } else {
    vi.doUnmock("@tauri-apps/api/core");
  }
  return import("./autostart");
}

const deleteInternals = () => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
};

beforeEach(() => {
  deleteInternals();
  vi.unstubAllGlobals();
});

describe("outside Tauri (plain browser)", () => {
  it("reports unavailable", async () => {
    const seam = await importSeam();
    expect(await seam.autostartStatus()).toEqual({ available: false });
  });

  it("refuses to pretend a toggle happened", async () => {
    const seam = await importSeam();
    expect(await seam.setAutostart(true)).toBe(false);
  });
});

describe("inside Tauri", () => {
  it("mirrors the host's enabled state", async () => {
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
    const invoke: Invoke = async (command) => (command === "autostart_get" ? true : undefined);
    const seam = await importSeam(invoke);
    expect(await seam.autostartStatus()).toEqual({ available: true, enabled: true });
  });

  it("reports a disabled host as available-but-off", async () => {
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
    const invoke: Invoke = async (command) => (command === "autostart_get" ? false : undefined);
    const seam = await importSeam(invoke);
    expect(await seam.autostartStatus()).toEqual({ available: true, enabled: false });
  });

  it("an undeterminable host state reads as unavailable", async () => {
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
    const invoke: Invoke = async () => null; // host answered Option::None
    const seam = await importSeam(invoke);
    expect(await seam.autostartStatus()).toEqual({ available: false });
  });

  it("a host refusal makes the toggle honest", async () => {
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
    const invoke: Invoke = async (command) => {
      if (command === "autostart_set") throw new Error("registry refused");
      return undefined;
    };
    const seam = await importSeam(invoke);
    expect(await seam.setAutostart(true)).toBe(false);
  });

  it("a successful toggle confirms", async () => {
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
    const seen: Array<[string, unknown]> = [];
    const invoke: Invoke = async (command, args) => {
      seen.push([command, args]);
      return undefined;
    };
    const seam = await importSeam(invoke);
    expect(await seam.setAutostart(false)).toBe(true);
    const [command, args] = seen[0] ?? [];
    expect(command).toBe("autostart_set");
    expect(args).toEqual({ enabled: false });
  });
});
