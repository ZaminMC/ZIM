// The settings page's discovery roots section (§64, ADR-0027/0029): the
// daemon's config is the truth — adds and removes ship the whole list and
// render the daemon's answer, a duplicate is refused before the wire, a
// refusal keeps the row and says why (§81), and the section says what the
// scan does with the roots.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsPage } from "./SettingsPage";
import { ProtocolRequestError } from "../../protocol/client";

vi.mock("../../state/actions", () => ({
  discoveryRoots: vi.fn(),
  setDiscoveryRoots: vi.fn(),
  storageGet: vi.fn(),
  storageSet: vi.fn(),
}));

import { discoveryRoots, setDiscoveryRoots, storageGet, storageSet } from "../../state/actions";

const rootsMock = discoveryRoots as ReturnType<typeof vi.fn>;
const setMock = setDiscoveryRoots as ReturnType<typeof vi.fn>;
const storageGetMock = storageGet as ReturnType<typeof vi.fn>;
const storageSetMock = storageSet as ReturnType<typeof vi.fn>;

beforeEach(() => {
  localStorage.clear();
  rootsMock.mockReset().mockResolvedValue({ roots: [] });
  setMock.mockReset().mockImplementation((roots: string[]) =>
    Promise.resolve({ roots }),
  );
  storageGetMock.mockReset().mockResolvedValue({
    root: "/home/u/.local/share/zim",
    defaultRoot: "/home/u/.local/share/zim",
    redirectActive: false,
    redirectPath: "/home/u/.config/zim/storage-location.txt",
    instancesDir: "/home/u/.local/share/zim/instances",
    cacheDir: "/home/u/.local/share/zim/cache",
    javaDir: "/home/u/.local/share/zim/java",
  });
  storageSetMock.mockReset();
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

describe("SettingsPage — storage (ADR-0034)", () => {
  it("maps where everything lives, with the real paths", async () => {
    storageGetMock.mockResolvedValue({
      root: "/d/ZIM",
      defaultRoot: "/home/u/.local/share/zim",
      redirectActive: true,
      redirectPath: "/home/u/.config/zim/storage-location.txt",
      instancesDir: "/d/ZIM/instances",
      cacheDir: "/d/ZIM/cache",
      javaDir: "/d/ZIM/java",
    });
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText("/d/ZIM")).toBeTruthy());
    expect(screen.getByText("/d/ZIM/instances")).toBeTruthy();
    expect(screen.getByText("/d/ZIM/cache")).toBeTruthy();
    expect(screen.getByText("/d/ZIM/java")).toBeTruthy();
    // Off the platform default → the redirect chip and the pointer's own
    // address are part of the honest answer.
    expect(screen.getByText("Redirected")).toBeTruthy();
    expect(
      screen.getByText(/redirect at \/home\/u\/\.config\/zim\/storage-location\.txt/),
    ).toBeTruthy();
  });

  it("the platform default is stated as a default — never a cage", async () => {
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText(/platform default/)).toBeTruthy());
    expect(screen.getByText(/point it elsewhere/)).toBeTruthy();
    expect(screen.queryByText("Redirected")).toBeNull();
  });

  it("setting a root ships the request and states the restart + not-moved facts", async () => {
    render(<SettingsPage />);
    const input = await screen.findByRole("textbox", { name: "New data root" });
    fireEvent.change(input, { target: { value: "  D:\\ZIM  " } });
    storageSetMock.mockResolvedValue({
      root: "D:\\ZIM",
      restartRequired: true,
      currentRoot: "/home/u/.local/share/zim",
      serversInCurrentRoot: 2,
    });
    // The read side refreshes with the pending root after the set.
    storageGetMock.mockResolvedValue({
      root: "/home/u/.local/share/zim",
      defaultRoot: "/home/u/.local/share/zim",
      redirectActive: false,
      redirectPath: "/home/u/.config/zim/storage-location.txt",
      pendingRoot: "D:\\ZIM",
      instancesDir: "/home/u/.local/share/zim/instances",
      cacheDir: "/home/u/.local/share/zim/cache",
      javaDir: "/home/u/.local/share/zim/java",
    });
    fireEvent.click(screen.getByRole("button", { name: /move data to/i }));
    await waitFor(() => expect(storageSetMock).toHaveBeenCalledWith("D:\\ZIM"));
    await waitFor(() =>
      expect(screen.getByText(/Restart ZIM — close it and open/)).toBeTruthy(),
    );
    // The left-behind count is named — the daemon never moves data.
    expect(screen.getByText(/2 registered servers stay in/)).toBeTruthy();
    // The pending root is the honest map's own field.
    expect(screen.getByText(/Pending: D:\\ZIM/)).toBeTruthy();
    // The draft is spent.
    expect((input as HTMLInputElement).value).toBe("");
  });

  it("a typed refusal is an error, not a silent success", async () => {
    render(<SettingsPage />);
    const input = await screen.findByRole("textbox", { name: "New data root" });
    fireEvent.change(input, { target: { value: "zim-data" } });
    storageSetMock.mockRejectedValue(
      new ProtocolRequestError({
        code: "PROTOCOL_INVALID_REQUEST",
        message: "The storage root \"zim-data\" is not an absolute path — the data root is a directory on this machine",
        remediation: [],
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: /move data to/i }));
    await waitFor(() => expect(screen.getByText(/not an absolute path/)).toBeTruthy());
    // The draft survives a refusal — the operator edits, not retypes.
    expect((input as HTMLInputElement).value).toBe("zim-data");
  });

  it("a failed read is an honest error, never a guessed map", async () => {
    storageGetMock.mockRejectedValue(new Error("the daemon is unreachable"));
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText(/unreachable/)).toBeTruthy());
    expect(screen.queryByRole("textbox", { name: "New data root" })).toBeNull();
  });
});
