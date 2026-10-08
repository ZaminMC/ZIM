// The crash card (§62): the founder's exact conversation — "Server
// stopped unexpectedly." with the honest reason, then [Restart],
// [View logs], [Ask Dutchmen]. Restart dispatches through the shared
// pending/actionError fields (one in-flight verb, one surfaced failure);
// the card resolves itself when the registry's next transition proves
// it stale. No cause is ever invented — no captured evidence means the
// card says so.

import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CrashCard, crashReason } from "./CrashCard";
import { useServers, type CrashInfo } from "../state/servers";
import { useUi } from "../state/ui";
import { startServer } from "../state/actions";

vi.mock("../state/actions", () => ({
  startServer: vi.fn(() => Promise.resolve({ state: "starting" })),
}));

function seedCrash(extra: Partial<CrashInfo> = {}) {
  useServers.getState().recordCrash({
    serverId: "survival",
    phase: "runtime",
    exitCode: 1,
    evidence: "java.lang.OutOfMemoryError: Metaspace",
    resolved: false,
    ...extra,
  });
}

describe("CrashCard", () => {
  afterEach(() => {
    cleanup();
    useServers.setState({ servers: {}, crashes: {} });
    useUi.setState({ pending: {}, actionErrors: {} });
    vi.clearAllMocks();
  });

  it("says the founder's sentence with the classified reason", () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);

    expect(screen.getByRole("status", { name: "Crash report" })).toBeTruthy();
    expect(screen.getByText("Server stopped unexpectedly.")).toBeTruthy();
    expect(screen.getByText(/Reason:/)).toBeTruthy();
    expect(screen.getByText("java.lang.OutOfMemoryError: Metaspace")).toBeTruthy();
    expect(screen.getByText("during runtime")).toBeTruthy();
    expect(screen.getByText("exit code: 1")).toBeTruthy();
  });

  it("never invents a cause: without evidence it says so", () => {
    seedCrash({ evidence: undefined });
    render(<CrashCard serverId="survival" />);
    expect(screen.getByText(/nothing was captured/)).toBeTruthy();
  });

  it("Restart dispatches the real verb through the shared pending fields", async () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));

    expect(useUi.getState().pending.survival).toBe("start");
    await vi.waitFor(() => expect(useUi.getState().pending.survival).toBeUndefined());
    expect(startServer).toHaveBeenCalledWith("survival");
  });

  it("a failed restart surfaces through the shared actionError fields", async () => {
    vi.mocked(startServer).mockRejectedValueOnce(new Error("boot refused"));
    seedCrash();
    render(<CrashCard serverId="survival" />);
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));

    await vi.waitFor(() => expect(useUi.getState().actionErrors.survival).toBeTruthy());
    expect(useUi.getState().actionErrors.survival?.message).toBe("boot refused");
    // The card stays: the crash is still the truth until the registry
    // transitions.
    expect(screen.queryByRole("status", { name: "Crash report" })).not.toBeNull();
  });

  it("View logs jumps to the log surface", () => {
    seedCrash();
    const onViewLogs = vi.fn();
    render(<CrashCard serverId="survival" onViewLogs={onViewLogs} />);
    fireEvent.click(screen.getByRole("button", { name: "View logs" }));
    expect(onViewLogs).toHaveBeenCalledOnce();
  });

  it("Ask Dutchmen is the reserved room: present, honest, inert", () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);
    const button = screen.getByRole("button", { name: "Ask Dutchmen" });
    expect(button.hasAttribute("disabled")).toBe(true);
    expect(button.getAttribute("title")).toMatch(/Reserved/);
  });

  it("hides when the operator dismisses", () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss crash report" }));
    expect(screen.queryByRole("status", { name: "Crash report" })).toBeNull();
  });

  it("hides once a later transition resolves the crash", () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);
    act(() => {
      useServers.getState().applyState("survival", "not-running");
    });
    expect(screen.queryByRole("status", { name: "Crash report" })).toBeNull();
  });

  it("details carrying a live state resolve the stale card too (upsert rule)", () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);
    act(() => {
      useServers.getState().upsert({
        serverId: "survival",
        displayName: "Survival",
        state: "running",
      });
    });
    expect(screen.queryByRole("status", { name: "Crash report" })).toBeNull();
  });

  it("renders nothing for a server without a crash", () => {
    render(<CrashCard serverId="quiet" />);
    expect(screen.queryByRole("status", { name: "Crash report" })).toBeNull();
  });

  it("still offers the backup recovery jump", () => {
    seedCrash();
    const onRecover = vi.fn();
    render(<CrashCard serverId="survival" onRecover={onRecover} />);
    fireEvent.click(screen.getByRole("button", { name: "Recover from a backup" }));
    expect(onRecover).toHaveBeenCalledOnce();
  });
});

describe("crashReason", () => {
  it("prefers the structured error message", () => {
    expect(
      crashReason({
        serverId: "s",
        phase: "runtime",
        error: { code: "E", message: "structured", remediation: [] },
        evidence: "first evidence line\nsecond",
        resolved: false,
      }),
    ).toBe("structured");
  });

  it("falls back to the first evidence line", () => {
    expect(
      crashReason({
        serverId: "s",
        phase: "runtime",
        evidence: "\njava.lang.InternalError\nmore",
        resolved: false,
      }),
    ).toBe("java.lang.InternalError");
  });

  it("returns null when nothing was captured — the card will not invent", () => {
    expect(
      crashReason({ serverId: "s", phase: "startup", resolved: false }),
    ).toBeNull();
  });
});
