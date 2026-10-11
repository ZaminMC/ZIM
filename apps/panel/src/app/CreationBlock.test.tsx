// The creation block: the Minecraft-style destroy overlay tied to REAL
// state. These tests pin the honesty laws — cracks advance with real
// progress, never heal, stop on failure, and the aria story matches.

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { CreationBlock } from "./CreationBlock";
import { progressView } from "../state/creationProgress";
import type { CreationProgressView } from "../state/creationProgress";

const viewOf = (progress: Parameters<typeof progressView>[0]): CreationProgressView =>
  progressView(progress);

afterEach(cleanup);

describe("CreationBlock", () => {
  it("an unstarted job is an unbroken block", () => {
    render(<CreationBlock view={viewOf(undefined)} succeeded={false} failed={false} />);
    expect(screen.getByRole("img")).toBeTruthy();
    expect(screen.getByRole("img").getAttribute("data-stage")).toBe("0");
  });

  it("the cracks deepen with real byte progress", () => {
    const { rerender } = render(
      <CreationBlock
        view={viewOf({ current: 0, total: 100, unit: "bytes", stage: "downloading" })}
        succeeded={false}
        failed={false}
      />,
    );
    const block = screen.getByRole("img");
    expect(block.getAttribute("data-stage")).toBe("3");

    rerender(
      <CreationBlock
        view={viewOf({ current: 50, total: 100, unit: "bytes", stage: "downloading" })}
        succeeded={false}
        failed={false}
      />,
    );
    expect(block.getAttribute("data-stage")).toBe("6");

    rerender(
      <CreationBlock
        view={viewOf({ current: 100, total: 100, unit: "bytes", stage: "downloading" })}
        succeeded={false}
        failed={false}
      />,
    );
    expect(block.getAttribute("data-stage")).toBe("9");
  });

  it("the cracks never heal when the total arrives late", () => {
    // 80% of a known total, then the same bytes with the total gone
    // (headers lied): the block holds its deepest crack.
    const { rerender } = render(
      <CreationBlock
        view={viewOf({ current: 80, total: 100, unit: "bytes", stage: "downloading" })}
        succeeded={false}
        failed={false}
      />,
    );
    const block = screen.getByRole("img");
    expect(block.getAttribute("data-stage")).toBe("8");
    rerender(
      <CreationBlock
        view={viewOf({ current: 80, unit: "bytes", stage: "downloading" })}
        succeeded={false}
        failed={false}
      />,
    );
    expect(block.getAttribute("data-stage")).toBe("8");
  });

  it("a stage regression never un-cracks the face (the running max)", () => {
    const { rerender } = render(
      <CreationBlock
        view={viewOf({ current: 100, total: 100, unit: "bytes", stage: "downloading" })}
        succeeded={false}
        failed={false}
      />,
    );
    const block = screen.getByRole("img");
    expect(block.getAttribute("data-stage")).toBe("9");
    // A late duplicate event at zero bytes (a retry's first callback)
    // must not heal the face.
    rerender(
      <CreationBlock
        view={viewOf({ current: 0, total: 100, unit: "bytes", stage: "downloading" })}
        succeeded={false}
        failed={false}
      />,
    );
    expect(block.getAttribute("data-stage")).toBe("9");
  });

  it("failure stops the block where it stopped, desaturated, no motion", () => {
    render(
      <CreationBlock
        view={viewOf({ current: 50, total: 100, unit: "bytes", stage: "downloading" })}
        succeeded={false}
        failed
      />,
    );
    const block = screen.getByRole("img");
    expect(block.getAttribute("data-stage")).toBe("6");
    expect(block.getAttribute("data-failed")).toBe("true");
    expect(screen.getByRole("img").getAttribute("aria-label")).toBe("Creation failed");
  });

  it("success carries the resolve state for the exit beat", () => {
    render(
      <CreationBlock
        view={viewOf({ stage: "registering", current: 0 })}
        succeeded
        failed={false}
      />,
    );
    const block = screen.getByRole("img");
    expect(block.getAttribute("data-succeeded")).toBe("true");
    expect(block.getAttribute("aria-label")).toBe("Server created");
  });

  it("the aria story states the real stage", () => {
    render(
      <CreationBlock
        view={viewOf({ current: 50, total: 100, unit: "bytes", stage: "downloading" })}
        succeeded={false}
        failed={false}
      />,
    );
    expect(screen.getByRole("img").getAttribute("aria-label")).toMatch(
      /crack stage 6 of 10/,
    );
  });
});
