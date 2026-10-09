// The labeled lifecycle pill: every state has a human word (the map is
// the vocabulary — an unmapped state shows its raw id rather than a lie),
// the transient states carry the live class, and the colors all flow
// from stateColor so the dot and the chip can never disagree.

import { render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { StatusChip } from "./StatusChip";
import styles from "./StatusChip.module.css";
import type { ServerState } from "../protocol/types";

afterEach(cleanup);

describe("<StatusChip />", () => {
  const words: Array<[ServerState, string]> = [
    ["running", "Running"],
    ["starting", "Starting"],
    ["stopping", "Stopping"],
    ["stopped", "Stopped"],
    ["not-running", "Not running"],
    ["failed-preflight", "Preflight failed"],
    ["crashed", "Crashed"],
    ["adopting", "Adopting"],
    ["unknown", "Unknown"],
  ];

  it.each(words)("state %s is worded %s", (state, word) => {
    render(<StatusChip state={state} />);
    expect(screen.getByText(word)).toBeTruthy();
  });

  it("an unmapped state degrades to its raw id, never a blank", () => {
    render(<StatusChip state={"mystery" as ServerState} />);
    expect(screen.getByText("mystery")).toBeTruthy();
  });

  it.each(["starting", "stopping", "adopting"] as ServerState[])(
    "%s carries the live class",
    (state) => {
      const { container } = render(<StatusChip state={state} />);
      expect(container.firstElementChild?.className).toContain(styles.live);
    },
  );

  it.each(["running", "stopped", "crashed"] as ServerState[])(
    "%s is not live",
    (state) => {
      const { container } = render(<StatusChip state={state} />);
      expect(container.firstElementChild?.className).not.toContain(styles.live);
    },
  );

  it("colors dot and pill from stateColor — one source, no drift", () => {
    vi.spyOn(window, "getComputedStyle").mockReturnValue({
      getPropertyValue: (key: string) => (key === "--state-running" ? "#1a7f37" : ""),
    } as unknown as CSSStyleDeclaration);
    const { container, unmount } = render(<StatusChip state="running" />);
    const chip = container.firstElementChild as HTMLElement;
    const dot = chip.firstElementChild as HTMLElement;
    // jsdom normalizes hex to rgb — the value, not the spelling, is the law.
    expect(dot.style.background).toBe("rgb(26, 127, 55)");
    expect(chip.style.color).toBe("rgb(26, 127, 55)");
    expect(chip.style.background).toContain("26, 127, 55");
    expect(chip.style.borderColor).toContain("26, 127, 55");
    unmount();
    vi.restoreAllMocks();
  });

  it("the dot is decoration — aria-hidden, the chip's text carries the meaning", () => {
    const { container } = render(<StatusChip state="stopped" />);
    const dot = container.firstElementChild?.firstElementChild as HTMLElement;
    expect(dot.getAttribute("aria-hidden")).toBe("true");
  });
});
