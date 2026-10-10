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

// -- The hover card's machine -----------------------------------------------
// The port of TabHoverCardController's visible laws, driven through the
// demo carrier: the delay law (no card under the floor), the slide law
// (no delay when a card is already up), the sniffer's kEvent (a click
// hides and re-arms the FULL delay), the 300ms reshow buffer, the strip
// leave's fade, and the chip's group card.

import { act } from "@testing-library/react";
import { vi } from "vitest";

describe("<FrameApp /> the hover card's machine", () => {
  beforeEach(() => {
    history.replaceState(null, "", "/frame.html?demo");
  });
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  async function bootStrip() {
    const view = render(<FrameApp />);
    await waitFor(() => {
      expect(view.container.querySelectorAll("[data-tab]").length).toBeGreaterThan(0);
    });
    return view;
  }

  // The earlier smoke tests mutate the module-scoped demo fixture (a
  // close, a create), so the machine tests never trust hard-coded ids —
  // they read the strip's live tabs and their visible titles.
  const liveTabIds = (container: HTMLElement): string[] =>
    [...container.querySelectorAll("[data-tab]")].map((e) => e.getAttribute("data-tab-id")!);

  const visibleTitle = (container: HTMLElement, id: string): string =>
    container.querySelector(`[data-tab-id='${id}'] .tab-title`)?.textContent ?? "";

  it("shows the card only after the delay law has run, title over domain", async () => {
    const { container } = await bootStrip();
    vi.useFakeTimers();
    const id = liveTabIds(container)[0];
    const title = container.querySelector<HTMLElement>(`[data-tab-id='${id}']`)!.getAttribute("aria-label")!.slice(4);
    act(() => {
      fireEvent.mouseEnter(container.querySelector<HTMLElement>(`[data-tab-id='${id}']`)!);
    });
    // The law has not elapsed — nothing shows, not even a stub.
    expect(container.querySelector(".hover-card")).toBeNull();
    act(() => {
      vi.advanceTimersByTime(299); // still under the 300ms floor
    });
    expect(container.querySelector(".hover-card")).toBeNull();
    act(() => {
      vi.advanceTimersByTime(2100); // any lawful delay has landed
    });
    const card = container.querySelector(".hover-card");
    expect(card).not.toBeNull();
    expect(card!.textContent).toContain(title);
  });

  it("slides instantly between tabs — no delay while a card is up", async () => {
    const { container } = await bootStrip();
    vi.useFakeTimers();
    const [firstId, secondId] = liveTabIds(container) as [string, string];
    act(() => {
      fireEvent.mouseEnter(container.querySelector<HTMLElement>(`[data-tab-id='${firstId}']`)!);
    });
    act(() => {
      vi.advanceTimersByTime(2100);
    });
    expect(container.querySelector(".hover-card")).not.toBeNull();
    act(() => {
      fireEvent.mouseEnter(container.querySelector<HTMLElement>(`[data-tab-id='${secondId}']`)!);
    });
    // Advance NOTHING: the card is already up, the content swaps now.
    const card = container.querySelector(".hover-card")!;
    expect(card.classList.contains("hover-card-sliding")).toBe(true);
    expect(card.textContent).toContain(visibleTitle(container, secondId));
  });

  it("a click hides the card and re-arms the full delay (kEvent)", async () => {
    const { container } = await bootStrip();
    vi.useFakeTimers();
    const [firstId, secondId] = liveTabIds(container) as [string, string];
    const first = container.querySelector<HTMLElement>(`[data-tab-id='${firstId}']`)!;
    act(() => {
      fireEvent.mouseEnter(first);
    });
    act(() => {
      vi.advanceTimersByTime(2100);
    });
    expect(container.querySelector(".hover-card")).not.toBeNull();
    // The sniffer hears the pointerdown: the card fades, and the next
    // show waits the full delay again (PreventImmediateReshow).
    act(() => {
      fireEvent.pointerDown(first);
    });
    // The demo carrier's fade is a state flip — synchronous in the
    // dispatch (no host, no animation clock in jsdom).
    expect(container.querySelector(".hover-card")!.classList.contains("hover-card-fading")).toBe(
      true,
    );
    act(() => {
      fireEvent.mouseEnter(container.querySelector<HTMLElement>(`[data-tab-id='${secondId}']`)!);
    });
    act(() => {
      vi.advanceTimersByTime(299);
    });
    // Under the demo carrier the fading node stays mounted until its
    // animation ends (jsdom never runs CSS) — the law to pin is that it
    // is STILL the old card, fading, and NOT the new tab's card: the
    // kEvent re-armed the full delay.
    const still = container.querySelector(".hover-card")!;
    expect(still.classList.contains("hover-card-fading")).toBe(true);
    expect(still.textContent).not.toContain(visibleTitle(container, secondId));
    act(() => {
      vi.advanceTimersByTime(2100);
    });
    const card = container.querySelector(".hover-card")!;
    expect(card.classList.contains("hover-card-fading")).toBe(false);
    expect(card.textContent).toContain(visibleTitle(container, secondId));
  });

  it("a re-entry within the 300ms buffer skips the delay", async () => {
    const { container } = await bootStrip();
    vi.useFakeTimers();
    const [firstId, secondId] = liveTabIds(container) as [string, string];
    act(() => {
      fireEvent.mouseEnter(container.querySelector<HTMLElement>(`[data-tab-id='${firstId}']`)!);
    });
    act(() => {
      vi.advanceTimersByTime(2100);
    });
    act(() => {
      fireEvent.mouseLeave(container.querySelector(".strip")!);
    });
    act(() => {
      fireEvent.mouseEnter(container.querySelector<HTMLElement>(`[data-tab-id='${secondId}']`)!);
    });
    // Advance NOTHING: the graze within the buffer shows immediately.
    const card = container.querySelector(".hover-card")!;
    expect(card.textContent).toContain(visibleTitle(container, secondId));
  });

  it("leaving the strip fades the card", async () => {
    const { container } = await bootStrip();
    vi.useFakeTimers();
    const tab = container.querySelector<HTMLElement>("[data-tab-id='1']")!;
    act(() => {
      fireEvent.mouseEnter(tab);
    });
    act(() => {
      vi.advanceTimersByTime(2100);
    });
    expect(container.querySelector(".hover-card")).not.toBeNull();
    act(() => {
      fireEvent.mouseLeave(container.querySelector(".strip")!);
    });
    expect(container.querySelector(".hover-card")!.classList.contains("hover-card-fading")).toBe(
      true,
    );
  });

  it("the chip's card is the group card — header and bullet members", async () => {
    const { container } = await bootStrip();
    vi.useFakeTimers();
    const chip = container.querySelector<HTMLElement>("[data-group-chip-id='1']")!;
    act(() => {
      fireEvent.mouseEnter(chip);
    });
    act(() => {
      vi.advanceTimersByTime(2100);
    });
    const card = container.querySelector(".hover-card")!;
    expect(card.classList.contains("hc-group")).toBe(true);
    expect(card.textContent).toContain("survival (2 tabs)");
    expect(card.textContent).toContain("•  Server survival");
    expect(card.textContent).toContain("•  Console hub");
  });
});
