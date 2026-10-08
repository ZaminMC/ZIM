// The settings page's discovery roots section (§64, ADR-0027/0029): the
// daemon's config is the truth — adds and removes ship the whole list and
// render the daemon's answer, a duplicate is refused before the wire, a
// refusal keeps the row and says why (§81), and the section says what the
// scan does with the roots.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsPage } from "./SettingsPage";

vi.mock("../../state/actions", () => ({
  discoveryRoots: vi.fn(),
  setDiscoveryRoots: vi.fn(),
}));

import { discoveryRoots, setDiscoveryRoots } from "../../state/actions";

const rootsMock = discoveryRoots as ReturnType<typeof vi.fn>;
const setMock = setDiscoveryRoots as ReturnType<typeof vi.fn>;

beforeEach(() => {
  localStorage.clear();
  rootsMock.mockReset().mockResolvedValue({ roots: [] });
  setMock.mockReset().mockImplementation((roots: string[]) =>
    Promise.resolve({ roots }),
  );
});

afterEach(cleanup);

describe("SettingsPage — discovery roots", () => {
  it("renders the daemon's roots as removable rows, and the add row", async () => {
    rootsMock.mockResolvedValue({ roots: ["/srv/minecraft", "D:\\boxes"] });
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText("/srv/minecraft")).toBeTruthy());
    expect(screen.getByText("D:\\boxes")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Remove root /srv/minecraft" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add root" })).toBeTruthy();
    // The section says what the scan does with the roots (§82 honesty).
    expect(screen.getByText(/never follows symlinks/)).toBeTruthy();
  });

  it("a failed read is a typed error, not a fake empty list", async () => {
    rootsMock.mockRejectedValue(new Error("the daemon is unreachable"));
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText(/unreachable/)).toBeTruthy());
    expect(screen.queryByRole("button", { name: "Add root" })).toBeNull();
  });

  it("adding a root ships the whole list and renders the daemon's answer", async () => {
    rootsMock.mockResolvedValue({ roots: ["/srv/minecraft"] });
    setMock.mockResolvedValue({ roots: ["/srv/minecraft", "/home/you/servers"] });
    render(<SettingsPage />);
    const input = await screen.findByRole("textbox", { name: "Add a discovery root" });
    fireEvent.change(input, { target: { value: "  /home/you/servers  " } });
    fireEvent.click(screen.getByRole("button", { name: "Add root" }));
    await waitFor(() =>
      expect(setMock).toHaveBeenCalledWith(["/srv/minecraft", "/home/you/servers"]),
    );
    // The row is the daemon's normalized answer, not the draft.
    await waitFor(() => expect(screen.getByText("/home/you/servers")).toBeTruthy());
    // And the draft is spent.
    expect((input as HTMLInputElement).value).toBe("");
  });

  it("a duplicate root is refused locally — no wire call, a note instead", async () => {
    rootsMock.mockResolvedValue({ roots: ["/srv/minecraft"] });
    render(<SettingsPage />);
    const input = await screen.findByRole("textbox", { name: "Add a discovery root" });
    fireEvent.change(input, { target: { value: "/srv/minecraft" } });
    fireEvent.click(screen.getByRole("button", { name: "Add root" }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toMatch(/already configured/));
    expect(setMock).not.toHaveBeenCalled();
  });

  it("removing a root ships the list without it and the row leaves", async () => {
    rootsMock.mockResolvedValue({ roots: ["/srv/minecraft", "/mnt/nas"] });
    setMock.mockResolvedValue({ roots: ["/mnt/nas"] });
    render(<SettingsPage />);
    await screen.findByText("/srv/minecraft");
    fireEvent.click(screen.getByRole("button", { name: "Remove root /srv/minecraft" }));
    await waitFor(() => expect(setMock).toHaveBeenCalledWith(["/mnt/nas"]));
    await waitFor(() => expect(screen.queryByText("/srv/minecraft")).toBeNull());
  });

  it("a refused set keeps the row and renders the typed failure (§81)", async () => {
    rootsMock.mockResolvedValue({ roots: ["/srv/minecraft"] });
    setMock.mockRejectedValue(new Error("the roots could not be saved"));
    render(<SettingsPage />);
    await screen.findByText("/srv/minecraft");
    fireEvent.click(screen.getByRole("button", { name: "Remove root /srv/minecraft" }));
    await waitFor(() => expect(screen.getByText(/could not be saved/)).toBeTruthy());
    // The row the daemon still holds is still on the page — nothing invented.
    expect(screen.getByText("/srv/minecraft")).toBeTruthy();
  });
});
