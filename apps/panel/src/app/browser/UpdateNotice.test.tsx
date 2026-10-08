// The update notice (ADR-0024): the chrome banner is silent exactly when
// nothing must be acted on, and every visible state carries the store's
// own sentence. The buttons ride the store's decisions — a restart click
// is a store restart, a retry is a manual check, a dismiss goes back to
// idle — so the banner has no policy of its own to test for.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { UpdateBackend, UpdateOffer } from "../../integration/updater";
import { UpdateNotice } from "./UpdateNotice";
import { setUpdateBackend, useUpdates } from "../../state/updates";

const offer: UpdateOffer = { version: "0.1.9", notes: "fixes" };

function fakeBackend(): UpdateBackend & {
  checks: number;
  installs: number;
  relaunches: number;
} {
  const backend = {
    checks: 0,
    installs: 0,
    relaunches: 0,
    currentVersion: async () => ({ ok: true as const, value: "0.1.4" }),
    check: async () => {
      backend.checks += 1;
      return { ok: true as const, value: null };
    },
    install: async () => {
      backend.installs += 1;
      return { ok: true as const, value: null };
    },
    relaunch: async () => {
      backend.relaunches += 1;
      return { ok: true as const, value: null };
    },
  };
  return backend;
}

describe("UpdateNotice", () => {
  afterEach(() => {
    cleanup();
    setUpdateBackend(null);
    useUpdates.setState({
      phase: { kind: "idle" },
      prefs: { autoCheck: true, autoInstall: false },
      installedVersion: "0.1.4",
    });
  });

  it("the quiet phases render nothing", () => {
    const phases = [
      { kind: "idle" as const },
      { kind: "unavailable" as const },
      { kind: "checking" as const },
      { kind: "upToDate" as const, version: "0.1.4" },
      { kind: "available" as const, offer },
    ];
    for (const phase of phases) {
      useUpdates.setState({ phase });
      const { container } = render(<UpdateNotice />);
      expect(container.childElementCount).toBe(0);
      cleanup();
    }
  });

  it("a running install is announced, with no button that could stomp it", () => {
    useUpdates.setState({ phase: { kind: "downloading", offer } });
    render(<UpdateNotice />);
    expect(screen.getByRole("status").textContent).toMatch(/0\.1\.9/);
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("a pending restart offers the restart, through the store", () => {
    const backend = fakeBackend();
    setUpdateBackend(backend);
    useUpdates.setState({ phase: { kind: "ready", offer } });
    render(<UpdateNotice />);
    expect(screen.getByRole("status").textContent).toMatch(/restart/);
    fireEvent.click(screen.getByRole("button", { name: "Restart now" }));
    expect(backend.relaunches).toBe(1);
  });

  it("a failure is an alert with a way out", () => {
    setUpdateBackend(fakeBackend());
    useUpdates.setState({
      phase: { kind: "error", message: "the update check failed: offline" },
    });
    render(<UpdateNotice />);
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toMatch(/offline/);

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(useUpdates.getState().phase).toEqual({ kind: "idle" });
  });

  it("the retry is a manual check — a good answer clears the alert on its own", async () => {
    const backend = fakeBackend();
    setUpdateBackend(backend);
    useUpdates.setState({
      phase: { kind: "error", message: "the update check failed: offline" },
      installedVersion: "0.1.4",
    });
    render(<UpdateNotice />);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    // The check resolves up-to-date, and the alert is gone — the honest
    // way, not by a fake "fixed" claim.
    await waitFor(() => {
      expect(useUpdates.getState().phase).toEqual({ kind: "upToDate", version: "0.1.4" });
    });
    expect(backend.checks).toBe(1);
  });
});
