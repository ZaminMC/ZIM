// New Server flow: client-side id sanity, required fields, and structured
// error display for daemon rejections.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NewServerModal, validateServerId } from "./NewServerModal";
import { useUi } from "../state/ui";

vi.mock("../state/wire", () => ({ client: {}, startWire: vi.fn() }));
vi.mock("../state/actions", () => ({
  registerServer: vi.fn(),
}));

import { registerServer } from "../state/actions";
import { ProtocolRequestError } from "../protocol/client";

describe("validateServerId", () => {
  it("accepts lowercase ids and rejects junk", () => {
    expect(validateServerId("survival")).toBeNull();
    expect(validateServerId("smp-2")).toBeNull();
    expect(validateServerId("")).toBeTypeOf("string");
    expect(validateServerId("Has Space")).toBeTypeOf("string");
    expect(validateServerId("-leading")).toBeTypeOf("string");
  });
});

describe("NewServerModal", () => {
  beforeEach(() => {
    useUi.setState({ newServerOpen: true });
    (registerServer as ReturnType<typeof vi.fn>).mockReset();
  });

  afterEach(cleanup);

  it("registers, upserts, opens the tab, and closes", async () => {
    (registerServer as ReturnType<typeof vi.fn>).mockResolvedValue({
      server: { serverId: "survival", displayName: "Survival", state: "not-running" },
    });
    render(<NewServerModal />);

    fireEvent.change(screen.getByLabelText(/server id/i), { target: { value: "survival" } });
    fireEvent.change(screen.getByLabelText(/display name/i), { target: { value: "Survival" } });
    fireEvent.change(screen.getByLabelText(/root directory/i), {
      target: { value: "/srv/mc/survival" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Register" }));

    await waitFor(() => expect(useUi.getState().newServerOpen).toBe(false));
    expect(useUi.getState().activeTab).toBe("survival");
    expect(useUi.getState().openTabs).toContain("survival");
  });

  it("surfaces a structured rejection with remediation", async () => {
    (registerServer as ReturnType<typeof vi.fn>).mockRejectedValue(
      new ProtocolRequestError({
        code: "SERVER_ID_EXISTS",
        message: "A server with this id is already registered.",
        remediation: ["Pick another id."],
      }),
    );
    render(<NewServerModal />);

    fireEvent.change(screen.getByLabelText(/server id/i), { target: { value: "dupe" } });
    fireEvent.change(screen.getByLabelText(/root directory/i), { target: { value: "/srv/mc" } });
    fireEvent.click(screen.getByRole("button", { name: "Register" }));

    await screen.findByRole("alert");
    expect(screen.getByText(/already registered/)).toBeTruthy();
    expect(screen.getByText("Pick another id.")).toBeTruthy();
    expect(useUi.getState().newServerOpen).toBe(true); // modal stays open
  });

  it("blocks submit with a client-side id error", () => {
    render(<NewServerModal />);
    fireEvent.change(screen.getByLabelText(/server id/i), { target: { value: "BAD ID" } });
    fireEvent.change(screen.getByLabelText(/root directory/i), { target: { value: "/srv" } });
    fireEvent.click(screen.getByRole("button", { name: "Register" }));
    expect(registerServer).not.toHaveBeenCalled();
    expect(screen.getByText(/lowercase letters/)).toBeTruthy();
  });
});
