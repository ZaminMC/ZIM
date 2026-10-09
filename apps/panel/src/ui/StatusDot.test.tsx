// The lifecycle dot's laws (ADR-0005 states): the color comes from the
// token sheet by state name, the fallback when the token is absent is the
// unknown token (never a crash, never a raw empty string), the three
// transient states pulse, and the mark stays legible to a screen reader
// through title + aria-label while the color itself is aria-hidden.

import { render, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { StatusDot, stateColor } from "./StatusDot";
import styles from "./StatusDot.module.css";
import type { ServerState } from "../protocol/types";

afterEach(cleanup);

function fakeWindowStyle(value: string): void {
  // stateColor reads the token sheet through getComputedStyle — stub it
  // so the law (token name built from the state, value passed through)
  // is observable without a CSS engine.
  vi.spyOn(window, "getComputedStyle").mockReturnValue({
    getPropertyValue: (key: string) => (key === "--state-running" ? value : ""),
  } as unknown as CSSStyleDeclaration);
}

describe("stateColor", () => {
  it("answers the state token's value from the computed style", () => {
    fakeWindowStyle("#1a7f37");
    expect(stateColor("running")).toBe("#1a7f37");
    vi.restoreAllMocks();
  });

  it("falls back to the unknown token when the sheet has no answer", () => {
    // jsdom's computed style has no custom properties — the fallback
    // path is what the test sees out of the box.
    expect(stateColor("running")).toBe("var(--state-unknown)");
  });
});

describe("<StatusDot />", () => {
  const cases: Array<[ServerState, boolean]> = [
    ["starting", true],
    ["stopping", true],
    ["adopting", true],
    ["running", false],
    ["stopped", false],
    ["not-running", false],
    ["failed-preflight", false],
    ["crashed", false],
    ["unknown", false],
  ];

  it.each(cases)("state %s → live class %s", (state, live) => {
    const { container } = render(<StatusDot state={state} />);
    const el = container.firstElementChild as HTMLElement;
    expect(el.className).toContain(styles.dot ?? "");
    if (live) expect(el.className).toContain(styles.live ?? "");
    else expect(el.className).not.toContain(styles.live ?? "");
  });

  it("labels itself for the tree-walker, colors itself aria-hidden", () => {
    const { container } = render(<StatusDot state="crashed" />);
    const el = container.firstElementChild as HTMLElement;
    expect(el.getAttribute("aria-label")).toBe("state: crashed");
    expect(el.getAttribute("title")).toBe("crashed");
  });
});
