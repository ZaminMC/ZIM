// The frame's render smoke: the frame document had NO test importing it,
// which is exactly how a syntax error could hide from every gate (vitest
// only transforms what tests import). This file keeps FrameApp in the
// transform graph and pins the visible chrome: tabs render from the
// model's slots, the active tab carries its surface class, close closes,
// new tab news.

import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { FrameApp } from "./FrameApp";

// The demo fixture: frameIpc's ?demo lane obeys the same command IDs the
// host does, so the smoke test runs the real view against the real
// command surface (no Tauri).
describe("<FrameApp /> against the demo fixture", () => {
  beforeEach(() => {
    history.replaceState(null, "", "/frame.html?demo");
  });
  afterEach(() => {
    cleanup();
  });

  it("boots the demo snapshot and renders the strip", async () => {
    const { container } = render(<FrameApp />);
    // The demo fixture boots with a running set of tabs (the fixture is
    // module-scoped, so tests only rely on relative counts).
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(0);
    });
    expect(screen.getByText("New tab")).toBeTruthy();
    expect(container.querySelector(".tab-active")?.textContent).toContain("New tab");
    expect(container.querySelector(".group-chip")?.textContent).toContain("survival");
  });

  it("closes a tab through the close button", async () => {
    const { container } = render(<FrameApp />);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(0);
    });
    const before = container.querySelectorAll("[data-tab]").length;
    const close = container.querySelector<HTMLElement>(".tab-close");
    expect(close).toBeTruthy();
    fireEvent.click(close!);
    // The demo command lane emits on a macrotask.
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBe(before - 1);
    });
  });

  it("opens a new tab through the + button", async () => {
    const { container } = render(<FrameApp />);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(0);
    });
    const before = container.querySelectorAll("[data-tab]").length;
    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBe(before + 1);
    });
  });
});
