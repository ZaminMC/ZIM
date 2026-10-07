// Players view regressions: the rosters render from the ping result,
// selecting a name opens the moderation cluster, a verb sends its console
// line over the same stdin path as the Console, ban confirms before it
// sends, everything is disabled while the server is not running, and a
// name the composer refuses never reaches the wire.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PlayersView } from "./PlayersView";
import type { PlayersListResult } from "../protocol/types";

vi.mock("../state/actions", () => ({
  listPlayers: vi.fn(),
  sendStdin: vi.fn(),
}));

import { listPlayers, sendStdin } from "../state/actions";

const listPlayersMock = listPlayers as ReturnType<typeof vi.fn>;
const sendStdinMock = sendStdin as ReturnType<typeof vi.fn>;

function resultOf(
  extra: Partial<Omit<PlayersListResult, "source">> = {},
): PlayersListResult {
  return {
    source: "ping",
    online: 1,
    max: 20,
    latencyMs: 12,
    sample: [{ id: "u1", name: "Notch" }],
    ...extra,
  };
}

beforeEach(() => {
  listPlayersMock.mockReset().mockResolvedValue(resultOf());
  sendStdinMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

describe("PlayersView moderation", () => {
  it("renders the roster and opens the moderation cluster on selection", async () => {
    render(<PlayersView serverId="demo" running={true} />);
    await waitFor(() => expect(listPlayersMock).toHaveBeenCalled());

    const notch = await screen.findByRole("button", { name: "Notch" });
    fireEvent.click(notch);

    expect(screen.getByRole("region", { name: "Actions for Notch" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Kick" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Op" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Deop" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Whitelist" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Ban" })).toBeTruthy();

    // Selecting again is a toggle, not a second panel.
    fireEvent.click(notch);
    expect(screen.queryByRole("region", { name: "Actions for Notch" })).toBeNull();
  });

  it("sends the composed line and says where the answer lands", async () => {
    render(<PlayersView serverId="demo" running={true} />);
    fireEvent.click(await screen.findByRole("button", { name: "Notch" }));

    fireEvent.click(screen.getByRole("button", { name: "Kick" }));

    await waitFor(() => expect(sendStdinMock).toHaveBeenCalledWith("demo", "kick Notch"));
    const note = await screen.findByText(/sent/i);
    expect(note.textContent).toContain("kick Notch");
    expect(note.textContent).toContain("answer lands in the log");
    // A fresh ping rides the send: the roster may answer the kick early.
    await waitFor(() => expect(listPlayersMock.mock.calls.length).toBeGreaterThanOrEqual(2));
  });

  it("confirms a ban before sending it", async () => {
    render(<PlayersView serverId="demo" running={true} />);
    fireEvent.click(await screen.findByRole("button", { name: "Notch" }));

    fireEvent.click(screen.getByRole("button", { name: "Ban" }));
    expect(sendStdinMock).not.toHaveBeenCalled();
    expect(screen.getByText(/until pardoned/i)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Yes, ban" }));
    await waitFor(() => expect(sendStdinMock).toHaveBeenCalledWith("demo", "ban Notch"));
  });

  it("keeps every verb disabled while the server is not running", async () => {
    render(<PlayersView serverId="demo" running={false} />);
    fireEvent.click(await screen.findByRole("button", { name: "Notch" }));

    for (const label of ["Kick", "Op", "Deop", "Whitelist", "Ban"]) {
      expect(screen.getByText<HTMLButtonElement>(label).disabled).toBe(true);
    }
    expect(screen.getByText(/must be running/i)).toBeTruthy();
    expect(sendStdinMock).not.toHaveBeenCalled();
  });

  it("refuses an illegal name honestly instead of sending it", async () => {
    // The ping sample is whatever the server answered; a modified server
    // can name a player with a space. The panel never composes that line.
    listPlayersMock.mockReset().mockResolvedValue(
      resultOf({ sample: [{ id: "u2", name: "bad name" }] }),
    );
    render(<PlayersView serverId="demo" running={true} />);
    fireEvent.click(await screen.findByRole("button", { name: "bad name" }));

    fireEvent.click(screen.getByRole("button", { name: "Kick" }));

    expect(sendStdinMock).not.toHaveBeenCalled();
    expect(screen.getByText(/not a legal Minecraft username/i)).toBeTruthy();
  });
});
