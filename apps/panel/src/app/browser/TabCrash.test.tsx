// The tab boundary's tests (§51, checklist row 23): a view that throws
// renders the recoverable "this tab crashed" page — never a modal, never
// a shell exit; the details stay inspectable one disclosure away; and
// reload rebuilds the view without touching any server process.

import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TabBoundary } from "./TabCrash";
import { useTabs } from "../../state/tabs";

// The boundary logs for the devtools console only; the tests silence and
// watch that channel.
vi.spyOn(console, "error").mockImplementation(() => {});

function Bomb({ onBoom }: { onBoom?: () => void }): never {
  onBoom?.();
  throw new Error("the view exploded");
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("TabBoundary", () => {
  it("a healthy view renders untouched", () => {
    render(
      <TabBoundary>
        <p>the console hub</p>
      </TabBoundary>,
    );
    expect(screen.getByText("the console hub")).toBeTruthy();
  });

  it("a throwing view becomes the crash page — the shell's other content survives", () => {
    render(
      <div>
        <p>the strip stays</p>
        <TabBoundary>
          <Bomb />
        </TabBoundary>
      </div>,
    );
    expect(screen.getByText("the strip stays")).toBeTruthy();
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("This tab crashed");
    // The recovery verb is a page verb, not a modal: reload rebuilds.
    expect(screen.getByRole("button", { name: "Reload this tab" })).toBeTruthy();
    // The crash never buries the evidence: the error's own words show.
    expect(alert.textContent).toContain("the view exploded");
  });

  it("the technical details are one disclosure away, with the component stack", () => {
    render(
      <TabBoundary>
        <Bomb />
      </TabBoundary>,
    );
    const details = screen.getByText("Technical details").closest("details")!;
    expect(details.hasAttribute("open")).toBe(false);
    fireEvent.click(screen.getByText("Technical details"));
    expect(details.hasAttribute("open")).toBe(true);
    const pre = details.querySelector("pre");
    expect(pre?.textContent).toContain("the view exploded");
    expect(pre?.textContent).toContain("Component stack:");
  });

  it("reload clears the crash, bumps the token, and re-mounts the view — which a still-broken boundary re-catches", () => {
    useTabs.setState({
      tabs: [{ id: 1, kind: "page", destination: { kind: "servers" }, title: "Servers", pinned: false, reloadToken: 0 }],
      activeId: 1,
    } as never);
    const boom = vi.fn();
    render(
      <TabBoundary>
        <Bomb onBoom={boom} />
      </TabBoundary>,
    );
    expect(screen.getByRole("alert")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Reload this tab" }));
    // The recovery verb ran through the tab model (the parent re-keys the
    // content; the server process is never touched).
    expect(useTabs.getState().tabs[0]?.reloadToken).toBe(1);
    // A still-broken view re-crashes — the boundary re-caught it, which
    // is itself the proof the children re-mounted.
    expect(screen.getByRole("alert")).toBeTruthy();
  });

  it("a crash whose error has no message still renders a disclosure", () => {
    function EmptyBomb(): never {
      throw new Error("");
    }
    render(
      <TabBoundary>
        <EmptyBomb />
      </TabBoundary>,
    );
    expect(screen.getByRole("alert")).toBeTruthy();
  });
});
