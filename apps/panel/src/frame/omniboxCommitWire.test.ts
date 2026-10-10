// The omnibox commit's WIRE law: the view's disposition rides the invoke
// intact — `new_tab` reaches shell_omnibox_commit, the clipboard helper
// reads through the plugin with the DOM clipboard as its honest fallback.
// (The view laws live in omniboxEditModel.test.tsx; this file pins what
// actually crosses the IPC boundary.)

import { describe, expect, it, vi, beforeEach } from "vitest";

const invoked: Array<{ cmd: string; args?: Record<string, unknown> }> = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    invoked.push({ cmd, args });
    return Promise.resolve({ kind: "navigated" });
  },
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: vi.fn(() => Promise.resolve("from the plugin")),
}));

import { clipboardText, omniboxCommit } from "./frameIpc";

describe("the omnibox commit wire (frameIpc)", () => {
  beforeEach(() => {
    invoked.length = 0;
    // The frame's own isTauri reads the window each call — the harness
    // raises the flag for the wire, and takes it back down after.
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });

  it("Alt-Enter's disposition rides the invoke as new_tab: true", async () => {
    await omniboxCommit("zim://settings/", true);
    expect(invoked[0]?.cmd).toBe("shell_omnibox_commit");
    expect(invoked[0]?.args).toMatchObject({
      text: "zim://settings/",
      new_tab: true,
    });
  });

  it("the plain commit does not ask for a new tab", async () => {
    await omniboxCommit("localhost:25565");
    expect(invoked[0]?.args).toMatchObject({
      text: "localhost:25565",
      new_tab: false,
    });
  });

  it("the clipboard text reads through the plugin", async () => {
    expect(await clipboardText()).toBe("from the plugin");
  });

  it("a denied clipboard falls back to the DOM clipboard, then to empty", async () => {
    // The plugin route dies (the capability's refusal shape)…
    const plugin = await import("@tauri-apps/plugin-clipboard-manager");
    vi.mocked(plugin.readText).mockRejectedValueOnce(new Error("denied"));
    // …the DOM clipboard answers.
    Object.defineProperty(navigator, "clipboard", {
      value: { readText: () => Promise.resolve("from the dom") },
      configurable: true,
    });
    expect(await clipboardText()).toBe("from the dom");
    // Both refused — an honest empty, never a throw.
    vi.mocked(plugin.readText).mockRejectedValueOnce(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", {
      value: {
        readText: () => Promise.reject(new Error("denied too")),
      },
      configurable: true,
    });
    expect(await clipboardText()).toBe("");
  });
});
