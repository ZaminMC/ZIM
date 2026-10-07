// The deferred list budget, enforced not asserted: a 2,000-entry
// collection commits its window (120 rows) with the listing, not the
// whole table; the remainder lands over idle frames; "Show all" is one
// explicit commit; and refreshed data resets the window for free. These
// tests run the setTimeout fallback (jsdom has no requestIdleCallback)
// under fake timers, so every slice is deterministic.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDeferredWindow } from "./deferred";

function Harness({ items, resetKey }: { items: number[]; resetKey?: string }) {
  const { visible, total, pending, done, showAll } = useDeferredWindow(items, resetKey);
  return (
    <div>
      <ul data-testid="list">
        {visible.map((value) => (
          <li key={value}>{value}</li>
        ))}
      </ul>
      <span data-testid="status">
        {`${visible.length}/${total} pending=${pending} done=${done}`}
      </span>
      <button onClick={showAll}>show all</button>
    </div>
  );
}

const THOUSANDS = Array.from({ length: 2000 }, (_, index) => index);

beforeEach(() => {
  // jsdom has no requestIdleCallback (the production fallback drives these
  // tests); delete it explicitly anyway so a future jsdom cannot silently
  // flip the scheduling under fake timers.
  delete (window as { requestIdleCallback?: unknown }).requestIdleCallback;
  delete (window as { cancelIdleCallback?: unknown }).cancelIdleCallback;
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

describe("useDeferredWindow", () => {
  it("commits the window with the listing, not the whole collection", () => {
    render(<Harness items={THOUSANDS} />);
    expect(screen.getByTestId("list").children.length).toBe(120);
    expect(screen.getByTestId("status").textContent).toBe(
      "120/2000 pending=1880 done=false",
    );
  });

  it("a collection shorter than the window is simply done", () => {
    render(<Harness items={[1, 2, 3]} />);
    expect(screen.getByTestId("list").children.length).toBe(3);
    expect(screen.getByTestId("status").textContent).toBe("3/3 pending=0 done=true");
  });

  it("catches up over idle frames, CHUNK at a time, and finishes", () => {
    render(<Harness items={THOUSANDS} />);
    // 120 → 360 → 600 … each idle frame adds CHUNK rows.
    act(() => {
      vi.advanceTimersByTime(16);
    });
    expect(screen.getByTestId("list").children.length).toBe(360);
    act(() => {
      vi.advanceTimersByTime(16);
    });
    expect(screen.getByTestId("list").children.length).toBe(600);

    // ceil(1880 / 240) = 8 slices in total; one extra beat for good measure.
    for (let beat = 0; beat < 9; beat += 1) {
      act(() => {
        vi.advanceTimersByTime(16);
      });
    }
    expect(screen.getByTestId("status").textContent).toBe(
      "2000/2000 pending=0 done=true",
    );
    expect(screen.getByTestId("list").children.length).toBe(2000);
  });

  it("show all renders everything in one explicit commit", () => {
    render(<Harness items={THOUSANDS} />);
    fireEvent.click(screen.getByRole("button", { name: "show all" }));
    expect(screen.getByTestId("list").children.length).toBe(2000);
    expect(screen.getByTestId("status").textContent).toBe(
      "2000/2000 pending=0 done=true",
    );
  });

  it("keeps the grown window across re-renders that carry no new data", () => {
    const { rerender } = render(<Harness items={THOUSANDS} />);
    act(() => {
      vi.advanceTimersByTime(16);
    });
    expect(screen.getByTestId("list").children.length).toBe(360);
    // The busy-flag flip: same array, same key, new render — the window
    // must not shrink back to the first-paint budget.
    rerender(<Harness items={THOUSANDS} />);
    expect(screen.getByTestId("list").children.length).toBe(360);
  });

  it("a fresh listing resets the window for free", () => {
    const { rerender } = render(<Harness items={THOUSANDS} />);
    act(() => {
      vi.advanceTimersByTime(16);
    });
    act(() => {
      vi.advanceTimersByTime(16);
    });
    expect(screen.getByTestId("list").children.length).toBe(600);

    // New data: the next commit already carries only the window — no
    // intermediate frame with the full old window over the new rows.
    const fresh = Array.from({ length: 2000 }, (_, index) => 10_000 + index);
    rerender(<Harness items={fresh} />);
    expect(screen.getByTestId("list").children.length).toBe(120);
    expect(screen.queryByText("10")).toBeNull();

    // A changed resetKey does the same even if the array were reused.
    rerender(<Harness items={fresh} resetKey="other" />);
    expect(screen.getByTestId("list").children.length).toBe(120);
  });
});
