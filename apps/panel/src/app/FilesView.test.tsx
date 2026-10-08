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
  copyFilesEntry: vi.fn(),
  searchFiles: vi.fn(),
  restartServer: vi.fn(),
}));

import {
  copyFilesEntry,
  deleteEntry,
  listFiles,
  mkdir,
  readWholeFile,
  renameEntry,
  restartServer,
  searchFiles,
  writeWholeFile,
} from "../state/actions";
import { ProtocolRequestError } from "../protocol/client";
import { useServers } from "../state/servers";

const listFilesMock = listFiles as ReturnType<typeof vi.fn>;
const readWholeFileMock = readWholeFile as ReturnType<typeof vi.fn>;
const writeWholeFileMock = writeWholeFile as ReturnType<typeof vi.fn>;
const mkdirMock = mkdir as ReturnType<typeof vi.fn>;
const renameEntryMock = renameEntry as ReturnType<typeof vi.fn>;
const deleteEntryMock = deleteEntry as ReturnType<typeof vi.fn>;
const copyFilesEntryMock = copyFilesEntry as ReturnType<typeof vi.fn>;
const restartServerMock = restartServer as ReturnType<typeof vi.fn>;
const searchFilesMock = searchFiles as ReturnType<typeof vi.fn>;

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
  copyFilesEntryMock.mockReset().mockResolvedValue({ path: "x", files: 1, bytes: 1 });
  searchFilesMock.mockReset().mockResolvedValue({ hits: [], truncated: false, scanned: 0 });
  restartServerMock.mockReset().mockResolvedValue({ serverId: "smp", state: "running", requestId: "r" });
  useServers.setState({
    servers: {
      smp: { serverId: "smp", displayName: "SMP", state: "running" },
    },
    crashes: {},
  });
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

  it("copies a row to the prompted destination and never overwrites silently", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("config.yml", "file", { sizeBytes: 4 })]));
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText("config.yml")).toBeTruthy());

    const prompt = vi.spyOn(window, "prompt").mockReturnValue("config copy.yml");
    fireEvent.click(screen.getByRole("button", { name: "copy" }));
    await waitFor(() => expect(copyFilesEntryMock).toHaveBeenCalledWith("smp", "config.yml", "config copy.yml"));

    // The typed refusal is the message — the panel says the honest word.
    copyFilesEntryMock.mockRejectedValue(
      new ProtocolRequestError({
        code: "FS_COPY_TARGET_EXISTS",
        message: "copy target already exists; copies never overwrite",
        remediation: [],
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "copy" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("FS_COPY_TARGET_EXISTS"));
    prompt.mockRestore();
  });

  it("searches the whole root, opens hits, and returns to the listing on clear", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("server.properties", "file", { sizeBytes: 11 })]));
    searchFilesMock.mockResolvedValue({
      hits: [
        { path: "plugins/EssentialsX/config.yml", kind: "file", sizeBytes: 40 },
        { path: "world", kind: "directory" },
      ],
      truncated: true,
      scanned: 900,
    });
    readWholeFileMock.mockResolvedValue(new TextEncoder().encode("x=1\n"));

    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText("server.properties")).toBeTruthy());

    const box = screen.getByLabelText(/Search file names/);
    fireEvent.change(box, { target: { value: "essentials" } });
    await waitFor(() => expect(searchFilesMock).toHaveBeenCalledWith("smp", "essentials"));
    await waitFor(() => expect(screen.getByText("config.yml")).toBeTruthy());
    expect(screen.getByText(/plugins\/EssentialsX/).textContent).toContain("plugins/EssentialsX");
    // The truncation is said, not hidden.
    expect(screen.getByText(/stopped at 2 matches/)).toBeTruthy();

    // Clearing the query returns the listing view.
    fireEvent.change(screen.getByLabelText(/Search file names/), { target: { value: "" } });
    await waitFor(() => expect(screen.getByText("server.properties")).toBeTruthy());
    // An empty query never reaches the wire — the listing is that view.
    expect(searchFilesMock).toHaveBeenLastCalledWith("smp", "essentials");

    // A file hit opens the editor at its real path.
    fireEvent.change(screen.getByLabelText(/Search file names/), { target: { value: "essentials" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "config.yml" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "config.yml" }));
    await waitFor(() =>
      expect(screen.getByLabelText(/Editing plugins\/EssentialsX\/config.yml/)).toBeTruthy(),
    );
  });

  it("downloads a file row through the browser's save affordance", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("world.dat", "file", { sizeBytes: 6 })]));
    readWholeFileMock.mockResolvedValue(new TextEncoder().encode("region"));
    const madeUrls: string[] = [];
    const revoke = vi.fn();
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn(() => {
        madeUrls.push("blob:fake");
        return "blob:fake";
      }),
      revokeObjectURL: revoke,
    });
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText("world.dat")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "download" }));
    await waitFor(() => expect(readWholeFileMock).toHaveBeenCalledWith("smp", "world.dat"));
    await waitFor(() => expect(madeUrls.length).toBe(1));
    vi.unstubAllGlobals();
  });

  it("refuses an oversized download before any read", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("huge.tar", "file", { sizeBytes: 200 * 1024 * 1024 })]));
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText("huge.tar")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "download" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("use a backup"));
    expect(readWholeFileMock).not.toHaveBeenCalled();
  });

  it("compose mode edits one value through the AST and saves only it", async () => {
    const raw = [
      "# Minecraft server properties",
      "server-port=25565",
      "online-mode=true",
      "motd=Hello World",
    ].join("\n");
    listFilesMock.mockResolvedValue(listing([entryOf("server.properties", "file", { sizeBytes: raw.length })]));
    readWholeFileMock.mockResolvedValue(new TextEncoder().encode(raw));
    writeWholeFileMock.mockResolvedValue(undefined);

    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByRole("button", { name: "server.properties" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "server.properties" }));
    await waitFor(() => expect(screen.getByLabelText(/Editing server.properties/)).toBeTruthy());

    // Source is the default; Compose is one click away and renders the
    // friendly controls.
    fireEvent.click(screen.getByRole("tab", { name: "Compose" }));
    await waitFor(() => expect(screen.getByText("Server Port")).toBeTruthy());

    // The boolean reads its own bytes: a real switch.
    const onlineMode = screen.getByRole("switch", { name: /online-mode/ });
    expect(onlineMode.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(onlineMode);
    expect(screen.getByRole("switch", { name: /online-mode/ }).getAttribute("aria-checked")).toBe("false");
    expect(screen.getByText("unsaved changes")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(writeWholeFileMock).toHaveBeenCalled());
    const savedBytes = vi.mocked(writeWholeFile).mock.calls[0]?.[2];
    if (!savedBytes) throw new Error("the save never wrote the file");
    const saved = new TextDecoder().decode(savedBytes);
    // One pair changed; the header comment and every other line survived.
    expect(saved).toContain("# Minecraft server properties");
    expect(saved).toContain("server-port=25565");
    expect(saved).toContain("online-mode=false");
    expect(saved).toContain("motd=Hello World");
  });

  it("save and restart is offered for boot files, behind its confirm", async () => {
    const raw = "online-mode=true\n";
    listFilesMock.mockResolvedValue(listing([entryOf("server.properties", "file", { sizeBytes: raw.length })]));
    readWholeFileMock.mockResolvedValue(new TextEncoder().encode(raw));
    writeWholeFileMock.mockResolvedValue(undefined);

    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByRole("button", { name: "server.properties" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "server.properties" }));
    await waitFor(() => expect(screen.getByLabelText(/Editing server.properties/)).toBeTruthy());

    // The honest sentence rides the bar.
    expect(screen.getByText(/read at boot/)).toBeTruthy();

    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    fireEvent.click(screen.getByRole("button", { name: "Save & Restart" }));
    await waitFor(() => expect(restartServerMock).toHaveBeenCalledWith("smp"));
    // The bytes landed first, then the restart — one honest order.
    expect(writeWholeFileMock).toHaveBeenCalled();
    confirm.mockRestore();
  });

  it("a stopped server gets the save, not a restart button", async () => {
    useServers.setState({
      servers: {
        ...useServers.getState().servers,
        smp: { serverId: "smp", displayName: "SMP", state: "stopped" },
      },
    });
    const raw = "online-mode=true\n";
    listFilesMock.mockResolvedValue(listing([entryOf("server.properties", "file", { sizeBytes: raw.length })]));
    readWholeFileMock.mockResolvedValue(new TextEncoder().encode(raw));

    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByRole("button", { name: "server.properties" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "server.properties" }));
    await waitFor(() => expect(screen.getByLabelText(/Editing server.properties/)).toBeTruthy());

    expect(screen.getByText(/read at boot/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Save & Restart" })).toBeNull();
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
