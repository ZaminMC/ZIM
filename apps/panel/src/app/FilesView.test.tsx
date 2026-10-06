// Files browser regressions: the listing renders with navigation, text
// files open in the editor and save through the staged upload, and
// outside-root symlinks are refused before any call.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FilesView } from "./FilesView";

vi.mock("../state/actions", () => ({
  listFiles: vi.fn(),
  readFileChunk: vi.fn(),
  writeFileChunk: vi.fn(),
  commitFile: vi.fn(),
  mkdir: vi.fn(),
  renameEntry: vi.fn(),
  deleteEntry: vi.fn(),
  readWholeFile: vi.fn(),
  writeWholeFile: vi.fn(),
}));

import {
  deleteEntry,
  listFiles,
  mkdir,
  readWholeFile,
  renameEntry,
  writeWholeFile,
} from "../state/actions";
import { ProtocolRequestError } from "../protocol/client";

const listFilesMock = listFiles as ReturnType<typeof vi.fn>;
const readWholeFileMock = readWholeFile as ReturnType<typeof vi.fn>;
const writeWholeFileMock = writeWholeFile as ReturnType<typeof vi.fn>;
const mkdirMock = mkdir as ReturnType<typeof vi.fn>;
const renameEntryMock = renameEntry as ReturnType<typeof vi.fn>;
const deleteEntryMock = deleteEntry as ReturnType<typeof vi.fn>;

type TestEntry = ReturnType<typeof entryOf>;

function listing(entries: TestEntry[]) {
  return { path: "plugins", total: entries.length, entries };
}

function entryOf(
  name: string,
  kind: "file" | "directory",
  extra: { sizeBytes?: number; symlinkOutside?: boolean } = {},
) {
  return { name, kind, modifiedMs: 1_730_803_200_000, ...extra };
}

beforeEach(() => {
  listFilesMock.mockReset();
  readWholeFileMock.mockReset();
  writeWholeFileMock.mockReset();
  mkdirMock.mockReset().mockResolvedValue(undefined);
  renameEntryMock.mockReset().mockResolvedValue(undefined);
  deleteEntryMock.mockReset().mockResolvedValue(undefined);
});

afterEach(cleanup);

describe("FilesView", () => {
  it("lists the directory with sizes and navigates into folders", async () => {
    // The mount listing is the root; "EssentialsX/" navigates into it.
    listFilesMock
      .mockResolvedValueOnce(listing([entryOf("EssentialsX", "directory"), entryOf("notes.txt", "file", { sizeBytes: 20 })]))
      .mockResolvedValueOnce(listing([entryOf("config.yml", "file", { sizeBytes: 5 })]));

    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText("notes.txt")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: /EssentialsX\// }));
    await waitFor(() => expect(listFilesMock).toHaveBeenLastCalledWith({ serverId: "smp", path: "EssentialsX", offset: 0, limit: 2000 }));
    await waitFor(() => expect(screen.getByText("config.yml")).toBeTruthy());
    expect(screen.getByText("EssentialsX")).toBeTruthy(); // breadcrumb
  });

  it("opens a text file, edits, and saves through the staged upload", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("server.properties", "file", { sizeBytes: 11 })]));
    readWholeFileMock.mockResolvedValue(new TextEncoder().encode("motd=hello\n"));
    writeWholeFileMock.mockResolvedValue(undefined);

    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByRole("button", { name: "server.properties" })).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "server.properties" }));
    await waitFor(() => expect(screen.getByLabelText(/Editing server.properties/)).toBeTruthy());
    expect(screen.getByText("saved")).toBeTruthy();

    const area = screen.getByLabelText<HTMLTextAreaElement>(/Editing server.properties/);
    fireEvent.change(area, { target: { value: "motd=hello world\n" } });
    expect(screen.getByText("unsaved changes")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(writeWholeFileMock).toHaveBeenCalledWith("smp", "server.properties", new TextEncoder().encode("motd=hello world\n")));
    await waitFor(() => expect(screen.getByText("saved")).toBeTruthy());
  });

  it("refuses outside-root symlinks before any daemon call", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("escape-link", "directory", { symlinkOutside: true })]));
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText(/escape-link/)).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: /escape-link/ }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("symlink outside the server root"));
    expect(listFilesMock).toHaveBeenCalledTimes(1); // no navigation happened
  });

  it("prompts for rename and delete actions", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("old.txt", "file", { sizeBytes: 1 })]));
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText("old.txt")).toBeTruthy());

    const prompt = vi.spyOn(window, "prompt").mockReturnValue("new.txt");
    fireEvent.click(screen.getByRole("button", { name: "rename" }));
    await waitFor(() => expect(renameEntryMock).toHaveBeenCalledWith("smp", "old.txt", "new.txt"));

    prompt.mockRestore();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    fireEvent.click(screen.getByRole("button", { name: "delete" }));
    await waitFor(() => expect(deleteEntryMock).toHaveBeenCalledWith("smp", "old.txt"));
    confirm.mockRestore();
  });

  it("surfaces typed errors from the daemon", async () => {
    listFilesMock.mockRejectedValue(
      new ProtocolRequestError({
        code: "FS_NOT_FOUND",
        message: "no such directory",
        remediation: [],
      }),
    );
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.getByRole("alert").textContent).toContain("FS_NOT_FOUND");
  });
});
