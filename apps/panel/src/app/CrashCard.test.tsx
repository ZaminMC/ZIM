// Crash card regression: the store keeps a resolved crash record (a later
// transition or the operator's acknowledgement flips `resolved`), so the
// card itself must hide on the flag — otherwise Acknowledge does nothing
// visible and the card outlives its welcome.

import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { CrashCard } from "./CrashCard";
import { useServers } from "../state/servers";

function seedCrash() {
  useServers.getState().recordCrash({
    serverId: "survival",
    phase: "runtime",
    exitCode: 1,
    evidence: "Encountered an unexpected exception",
    resolved: false,
  });
}

describe("CrashCard", () => {
  afterEach(() => {
    cleanup();
    useServers.setState({ servers: {}, crashes: {} });
  });

  it("renders the phase, exit code, and evidence excerpt", () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);

    expect(screen.getByRole("status", { name: "Crash report" })).toBeTruthy();
    expect(screen.getByText("The server crashed during runtime")).toBeTruthy();
    expect(screen.getByText("exit code: 1")).toBeTruthy();
    expect(screen.getByText("Encountered an unexpected exception")).toBeTruthy();
  });

  it("hides when the operator acknowledges", () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);

    fireEvent.click(screen.getByRole("button", { name: "Acknowledge" }));
    expect(screen.queryByRole("status", { name: "Crash report" })).toBeNull();
  });

  it("hides when the operator dismisses via the × button", () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);

    fireEvent.click(screen.getByRole("button", { name: "Dismiss crash report" }));
    expect(screen.queryByRole("status", { name: "Crash report" })).toBeNull();
  });

  it("stays hidden once a later transition resolves the crash", () => {
    seedCrash();
    render(<CrashCard serverId="survival" />);
    act(() => {
      useServers.getState().applyState("survival", "not-running");
    });

    expect(screen.queryByRole("status", { name: "Crash report" })).toBeNull();
  });

  it("renders nothing for a server without a crash", () => {
    render(<CrashCard serverId="quiet" />);
    expect(screen.queryByRole("status", { name: "Crash report" })).toBeNull();
  });
});
