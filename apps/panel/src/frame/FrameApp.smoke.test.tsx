// The frame's render smoke: the frame document had NO test importing it,
// which is exactly how a syntax error could hide from every gate (vitest
// only transforms what tests import). This file keeps FrameApp in the
// transform graph and pins the visible chrome: tabs render from the
// model's slots, the active tab carries its surface class, close closes,
// new tab news.

import { describe, expect, it, beforeEach, afterEach } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
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
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(
        0,
      );
    });
    expect(screen.getByText("New tab")).toBeTruthy();
    expect(container.querySelector(".tab-active")?.textContent).toContain(
      "New tab",
    );
    expect(container.querySelector(".group-chip")?.textContent).toContain(
      "survival",
    );
  });

  it("closes a tab on the PRESS of the close button (Chromium's law)", async () => {
    const { container } = render(<FrameApp />);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(
        0,
      );
    });
    const before = container.querySelectorAll("[data-tab]").length;
    const close = container.querySelector<HTMLElement>(".tab-close");
    expect(close).toBeTruthy();
    // The close rides the POINTERDOWN (TabStrip closes from
    // Tab::OnMousePressed) — a click race can never eat it, and the
    // press never arms a drag.
    fireEvent.pointerDown(close!, { button: 0 });
    // The demo command lane emits on a macrotask.
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBe(before - 1);
    });
  });

  it("opens a new tab through the + button", async () => {
    const { container } = render(<FrameApp />);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(
        0,
      );
    });
    const before = container.querySelectorAll("[data-tab]").length;
    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBe(before + 1);
    });
  });

  it("wears the real brand mark for the software that has one", async () => {
    const { container } = render(<FrameApp />);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(
        0,
      );
    });
    // survival runs paper (softwareMarks.ts) — the official mark rides
    // as an <img>, not a monogram span. (The fixture's other tabs point
    // at marked softwares too; the monogram fallback is pinned at the
    // marks module's level.)
    const imgs = container.querySelectorAll("img.favicon-mark.is-img");
    expect(imgs.length).toBeGreaterThan(0);
  });

  it("ctrl+press toggles the selection on and off (Tab::OnMousePressed's ctrl branch)", async () => {
    const { container } = render(<FrameApp />);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(
        0,
      );
    });
    // The boot's emit storm settles first — a press inside it races the
    // hello snapshot, and the law under test is the toggle, not the boot.
    await new Promise((resolve) => setTimeout(resolve, 150));
    // A NON-active tab: the ctrl press adds it to the selection, the
    // paint wears the selected fill.
    const tabs = Array.from(
      container.querySelectorAll<HTMLElement>("[data-tab]"),
    );
    const target = tabs.find(
      (t) =>
        !t.className.includes("tab-active") &&
        !t.className.includes("tab-pinned"),
    );
    expect(target).toBeTruthy();
    // jsdom has no PointerEvent — fireEvent.pointerDown degrades to a
    // bare Event with NO button and NO modifiers. The press the law
    // needs is a MouseEvent carrying the modifier (the same stand-in
    // the close button's press pins; React reads the nativeEvent).
    const press = (el: HTMLElement, ctrl = true) =>
      el.dispatchEvent(
        new MouseEvent("pointerdown", {
          bubbles: true,
          cancelable: true,
          button: 0,
          ctrlKey: ctrl,
        }),
      );
    press(target!);
    // The demo command lane emits on a macrotask.
    await waitFor(() => {
      expect(
        container.querySelectorAll(".tab-inactive.tab-selected").length,
      ).toBe(1);
    });
    // The second ctrl press DESELECTS it (the selection had company —
    // the active never leaves alone).
    press(target!);
    await waitFor(() => {
      expect(
        container.querySelectorAll(".tab-inactive.tab-selected").length,
      ).toBe(0);
    });
  });

  // jsdom has no PointerEvent — every session event below rides a real
  // MouseEvent stand-in (button, coordinates, modifiers all carried).
  const sessionPress = (el: HTMLElement, opts: PointerEventInit = {}) =>
    el.dispatchEvent(
      new MouseEvent("pointerdown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        ...opts,
      }),
    );
  const sessionMove = (x: number, y = 12) =>
    window.dispatchEvent(
      new MouseEvent("pointermove", {
        bubbles: true,
        cancelable: true,
        button: 0,
        clientX: x,
        clientY: y,
      }),
    );
  const sessionUp = (x: number, y = 12) =>
    window.dispatchEvent(
      new MouseEvent("pointerup", {
        bubbles: true,
        cancelable: true,
        button: 0,
        clientX: x,
        clientY: y,
      }),
    );

  it("a selected source drags the WHOLE selection as one block (MoveSelectedTabsTo)", async () => {
    const { container } = render(<FrameApp />);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(
        0,
      );
    });
    await new Promise((resolve) => setTimeout(resolve, 150));
    // The selection: the boot's active plus one ctrl-added tab.
    const tabs = () =>
      Array.from(container.querySelectorAll<HTMLElement>("[data-tab]"));
    const target = tabs().find(
      (t) =>
        !t.className.includes("tab-active") &&
        !t.className.includes("tab-pinned"),
    );
    expect(target).toBeTruthy();
    target!.dispatchEvent(
      new MouseEvent("pointerdown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        ctrlKey: true,
      }),
    );
    await waitFor(() => {
      expect(
        container.querySelectorAll(".tab-inactive.tab-selected").length,
      ).toBe(1);
    });
    // The press on the selected source keeps the selection (the press
    // law) — a real drag then carries EVERY selected tab: both wear the
    // lifted posture while the session lives.
    const rect = target!.getBoundingClientRect();
    sessionPress(target!, { clientX: rect.x + 10, clientY: rect.y + 10 });
    sessionMove(rect.x + 80);
    await waitFor(() => {
      expect(container.querySelectorAll(".tab-dragging").length).toBe(2);
    });
    // The drop lands the block: the two selected tabs end up ADJACENT
    // in the strip (they were apart), and the selection survives.
    const idOf = (el: HTMLElement) => el.dataset.tabId;
    const beforePair = tabs()
      .filter((t) => t.className.includes("tab-selected") || t.className.includes("tab-active"))
      .map(idOf);
    sessionUp(rect.x + 80);
    await waitFor(() => {
      const after = tabs()
        .filter(
          (t) =>
            t.className.includes("tab-selected") ||
            t.className.includes("tab-active"),
        )
        .map(idOf);
      const positions = after.map((id) =>
        tabs().findIndex((t) => idOf(t) === id),
      );
      expect(beforePair.length).toBeGreaterThan(0);
      const span = (positions[positions.length - 1] ?? 0) - (positions[0] ?? 0);
      expect(span).toBe(positions.length - 1);
    });
    expect(container.querySelectorAll(".tab-dragging").length).toBe(0);
  });

  it("a ctrl-DESELECTed source refuses the drag (Tab::OnMousePressed's return false)", async () => {
    const { container } = render(<FrameApp />);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(
        0,
      );
    });
    await new Promise((resolve) => setTimeout(resolve, 150));
    const ctrlPress = (el: HTMLElement) =>
      el.dispatchEvent(
        new MouseEvent("pointerdown", {
          bubbles: true,
          cancelable: true,
          button: 0,
          ctrlKey: true,
        }),
      );
    // Build a selection, then deselect the added tab: the drag press is
    // the SAME press that removed it — the source stands outside its
    // selection at the threshold, so the session dies before it began.
    const tabs = () =>
      Array.from(container.querySelectorAll<HTMLElement>("[data-tab]"));
    const target = tabs().find(
      (t) =>
        !t.className.includes("tab-active") &&
        !t.className.includes("tab-pinned"),
    );
    expect(target).toBeTruthy();
    ctrlPress(target!);
    await waitFor(() => {
      expect(
        container.querySelectorAll(".tab-inactive.tab-selected").length,
      ).toBe(1);
    });
    const orderBefore = tabs().map((t) => t.dataset.tabId);
    ctrlPress(target!); // the deselect — the drag press itself
    await new Promise((resolve) => setTimeout(resolve, 60)); // the emit lands
    const rect = target!.getBoundingClientRect();
    sessionPress(target!, { clientX: rect.x + 10, clientY: rect.y + 10 });
    sessionMove(rect.x + 90);
    sessionUp(rect.x + 90);
    await new Promise((resolve) => setTimeout(resolve, 80));
    // No drag posture ever painted, no reorder happened.
    expect(container.querySelectorAll(".tab-dragging").length).toBe(0);
    expect(tabs().map((t) => t.dataset.tabId)).toEqual(orderBefore);
  });

  it("the plain release without a drag collapses a standing multi-selection (OnMouseReleased)", async () => {
    const { container } = render(<FrameApp />);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-tab]").length).toBeGreaterThan(
        0,
      );
    });
    await new Promise((resolve) => setTimeout(resolve, 150));
    const tabs = () =>
      Array.from(container.querySelectorAll<HTMLElement>("[data-tab]"));
    const target = tabs().find(
      (t) =>
        !t.className.includes("tab-active") &&
        !t.className.includes("tab-pinned"),
    );
    expect(target).toBeTruthy();
    // The multi-selection stands.
    target!.dispatchEvent(
      new MouseEvent("pointerdown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        ctrlKey: true,
      }),
    );
    await waitFor(() => {
      expect(
        container.querySelectorAll(".tab-inactive.tab-selected").length,
      ).toBe(1);
    });
    // A plain press keeps the selection; the plain RELEASE (a click that
    // never became a drag) collapses it to the clicked tab — and that
    // tab becomes the active. (jsdom never synthesizes a click from a
    // programmatic pointer pair — the release's click rides explicitly,
    // exactly what the browser dispatches on the release.)
    const rect = target!.getBoundingClientRect();
    sessionPress(target!, { clientX: rect.x + 10, clientY: rect.y + 10 });
    sessionUp(rect.x + 10, rect.y + 10);
    target!.dispatchEvent(
      new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 }),
    );
    await waitFor(() => {
      expect(
        container.querySelectorAll(".tab-inactive.tab-selected").length,
      ).toBe(0);
      expect(target!.className.includes("tab-active")).toBe(true);
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
      expect(
        view.container.querySelectorAll("[data-tab]").length,
      ).toBeGreaterThan(0);
    });
    return view;
  }

  // The earlier smoke tests mutate the module-scoped demo fixture (a
  // close, a create), so the machine tests never trust hard-coded ids —
  // they read the strip's live tabs and their visible titles.
  const liveTabIds = (container: HTMLElement): string[] =>
    [...container.querySelectorAll("[data-tab]")].map((e) =>
      e.getAttribute("data-tab-id")!,
    );

  const visibleTitle = (container: HTMLElement, id: string): string =>
    container.querySelector(`[data-tab-id='${id}'] .tab-title`)?.textContent ??
    "";

  it("shows the card only after the delay law has run, title over domain", async () => {
    const { container } = await bootStrip();
    vi.useFakeTimers();
    const id = liveTabIds(container)[0];
    const title = container
      .querySelector<HTMLElement>(`[data-tab-id='${id}']`)!
      .getAttribute("aria-label")!
      .slice(4);
    act(() => {
      fireEvent.mouseEnter(
        container.querySelector<HTMLElement>(`[data-tab-id='${id}']`)!,
      );
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
      fireEvent.mouseEnter(
        container.querySelector<HTMLElement>(`[data-tab-id='${firstId}']`)!,
      );
    });
    act(() => {
      vi.advanceTimersByTime(2100);
    });
    expect(container.querySelector(".hover-card")).not.toBeNull();
    act(() => {
      fireEvent.mouseEnter(
        container.querySelector<HTMLElement>(`[data-tab-id='${secondId}']`)!,
      );
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
    const first = container.querySelector<HTMLElement>(
      `[data-tab-id='${firstId}']`,
    )!;
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
    expect(
      container
        .querySelector(".hover-card")!
        .classList.contains("hover-card-fading"),
    ).toBe(true);
    act(() => {
      fireEvent.mouseEnter(
        container.querySelector<HTMLElement>(`[data-tab-id='${secondId}']`)!,
      );
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
      fireEvent.mouseEnter(
        container.querySelector<HTMLElement>(`[data-tab-id='${firstId}']`)!,
      );
    });
    act(() => {
      vi.advanceTimersByTime(2100);
    });
    act(() => {
      fireEvent.mouseLeave(container.querySelector(".strip")!);
    });
    act(() => {
      fireEvent.mouseEnter(
        container.querySelector<HTMLElement>(`[data-tab-id='${secondId}']`)!,
      );
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
    expect(
      container
        .querySelector(".hover-card")!
        .classList.contains("hover-card-fading"),
    ).toBe(true);
  });

  it("the chip's card is the group card — header and bullet members", async () => {
    const { container } = await bootStrip();
    vi.useFakeTimers();
    const chip = container.querySelector<HTMLElement>(
      "[data-group-chip-id='1']",
    )!;
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
