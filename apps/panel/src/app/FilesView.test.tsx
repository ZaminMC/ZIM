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

  it("asks for rename and delete in the app's own dialogs", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("old.txt", "file", { sizeBytes: 1 })]));
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText("old.txt")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "rename" }));
    const dialog = await waitFor(() => screen.getByRole("dialog"));
    expect(dialog.textContent).toContain('Rename "old.txt"');
    const nameBox = await waitFor(() =>
      screen.getByRole<HTMLInputElement>("textbox", { name: "New name" }),
    );
    expect(nameBox.value).toBe("old.txt"); // prefilled with the current name
    fireEvent.change(nameBox, { target: { value: "new.txt" } });
    fireEvent.submit(nameBox.closest("form")!);
    await waitFor(() => expect(renameEntryMock).toHaveBeenCalledWith("smp", "old.txt", "new.txt"));

    fireEvent.click(screen.getByRole("button", { name: "delete" }));
    await waitFor(() => expect(screen.getByRole("dialog").textContent).toContain("This cannot be undone."));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(deleteEntryMock).toHaveBeenCalledWith("smp", "old.txt"));
  });

  it("the rename dialog refuses an unchanged name", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("old.txt", "file", { sizeBytes: 1 })]));
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText("old.txt")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "rename" }));
    const nameBox = await waitFor(() =>
      screen.getByRole<HTMLInputElement>("textbox", { name: "New name" }),
    );
    fireEvent.submit(nameBox.closest("form")!); // same name submitted
    const alert = await waitFor(() => screen.getByRole("alert"));
    expect(alert.textContent).toContain("the same as the current one");
    expect(renameEntryMock).not.toHaveBeenCalled();
  });

  it("copies a row to the asked destination and never overwrites silently", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("config.yml", "file", { sizeBytes: 4 })]));
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByText("config.yml")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "copy" }));
    const toBox = await waitFor(() =>
      screen.getByRole<HTMLInputElement>("textbox", { name: "Destination path" }),
    );
    expect(toBox.value).toBe("config copy.yml"); // the suggested destination
    fireEvent.change(toBox, { target: { value: "config copy.yml" } });
    fireEvent.submit(toBox.closest("form")!);
    await waitFor(() =>
      expect(copyFilesEntryMock).toHaveBeenCalledWith("smp", "config.yml", "config copy.yml"),
    );

    // The typed refusal is the message — the panel says the honest word.
    copyFilesEntryMock.mockRejectedValue(
      new ProtocolRequestError({
        code: "FS_COPY_TARGET_EXISTS",
        message: "copy target already exists; copies never overwrite",
        remediation: [],
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "copy" }));
    const again = await waitFor(() =>
      screen.getByRole<HTMLInputElement>("textbox", { name: "Destination path" }),
    );
    fireEvent.submit(again.closest("form")!);
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("FS_COPY_TARGET_EXISTS"));
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

  it("the compose mode is a claim about ONE file — it never leaks onto the next", async () => {
    // Compose's home is server.properties; the operator leaves for a
    // config.yml through the breadcrumb. The stuck mode used to parse
    // the YAML as properties — and an edit would re-render its lines in
    // `key=value` syntax, corrupting the file on save. The mode choice
    // must restart with the file (the comment's own law).
    const props = "server-port=25565\nmotd=Hello\n";
    const yaml = "server-port: 25565\nmotd: Hello\n";
    listFilesMock
      .mockResolvedValueOnce(listing([entryOf("server.properties", "file", { sizeBytes: props.length })]))
      .mockResolvedValueOnce(
        listing([
          entryOf("server.properties", "file", { sizeBytes: props.length }),
          entryOf("config.yml", "file", { sizeBytes: yaml.length }),
        ]),
      );
    readWholeFileMock
      .mockResolvedValueOnce(new TextEncoder().encode(props))
      .mockResolvedValueOnce(new TextEncoder().encode(yaml));

    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByRole("button", { name: "server.properties" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "server.properties" }));
    await waitFor(() => expect(screen.getByLabelText(/Editing server.properties/)).toBeTruthy());

    // Compose on — the mode's home file.
    fireEvent.click(screen.getByRole("tab", { name: "Compose" }));
    await waitFor(() => expect(screen.getByText("Server Port")).toBeTruthy());

    // Leave through the breadcrumb (no dirty draft stands in the way),
    // then open the YAML.
    fireEvent.click(screen.getByRole("button", { name: "root" }));
    await waitFor(() => expect(screen.getByText("config.yml")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "config.yml" }));

    // The YAML opens in Source — the textarea is the editor, and the
    // compose surface (whose parse would wear the file's `:` syntax as
    // pairs) never renders for it.
    await waitFor(() => expect(screen.getByLabelText(/Editing config.yml/)).toBeTruthy());
    expect(screen.queryByText("Server Port")).toBeNull();
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

    fireEvent.click(screen.getByRole("button", { name: "Save & Restart" }));
    // The application's own confirm — the native window.confirm is gone.
    await waitFor(() => expect(screen.getByRole("dialog").textContent).toContain("players will be disconnected"));
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));
    await waitFor(() => expect(restartServerMock).toHaveBeenCalledWith("smp"));
    // The bytes landed first, then the restart — one honest order.
    expect(writeWholeFileMock).toHaveBeenCalled();
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

  it("a failed verb shows the daemon's own sentence, never a template placeholder", async () => {
    listFilesMock.mockResolvedValue(listing([entryOf("old.txt", "file", { sizeBytes: 3 })]));
    deleteEntryMock.mockRejectedValueOnce(
      new ProtocolRequestError({
        code: "FS_IS_DIRECTORY",
        message: "the path is a directory",
        remediation: [],
      }),
    );
    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByRole("button", { name: "old.txt" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" })); // the dialog's confirm
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    const alert = screen.getByRole("alert").textContent ?? "";
    // The interpolated sentence carries the daemon's message — the old
    // bug printed the literal text "described.title" to operators.
    expect(alert).toContain("the path is a directory");
    expect(alert).not.toContain("described.title");
  });

  it("compose shows a duplicated key once, as its last value, and edits the winner", async () => {
    const raw = ["motd=first", "motd=second", "plain=1"].join("\n");
    listFilesMock.mockResolvedValue(listing([entryOf("server.properties", "file", { sizeBytes: raw.length })]));
    readWholeFileMock.mockResolvedValue(new TextEncoder().encode(raw));
    writeWholeFileMock.mockResolvedValue(undefined);

    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByRole("button", { name: "server.properties" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "server.properties" }));
    await waitFor(() => expect(screen.getByLabelText(/Editing server.properties/)).toBeTruthy());
    fireEvent.click(screen.getByRole("tab", { name: "Compose" }));

    // ONE row carries the key, holding the LAST value — the one the
    // server's own loader reads (Java's properties semantics).
    await waitFor(() => expect(screen.getByDisplayValue("second")).toBeTruthy());
    expect(screen.queryByDisplayValue("first")).toBeNull();
    // The honest note says the duplicate exists.
    expect(screen.getByText(/duplicated key shows/)).toBeTruthy();

    // The edit lands on the winning line; the shadowed copy keeps its
    // bytes for Source to show.
    fireEvent.change(screen.getByDisplayValue("second"), { target: { value: "third" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(writeWholeFileMock).toHaveBeenCalled());
    const savedBytes = vi.mocked(writeWholeFile).mock.calls[0]?.[2];
    if (!savedBytes) throw new Error("the save never wrote the file");
    const saved = new TextDecoder().decode(savedBytes);
    expect(saved).toBe(["motd=first", "motd=third", "plain=1"].join("\n"));
  });

  it("closing a dirty editor asks before discarding; cancel keeps the draft", async () => {
    const raw = "motd=hello\n";
    listFilesMock.mockResolvedValue(listing([entryOf("server.properties", "file", { sizeBytes: raw.length })]));
    readWholeFileMock.mockResolvedValue(new TextEncoder().encode(raw));

    render(<FilesView serverId="smp" />);
    await waitFor(() => expect(screen.getByRole("button", { name: "server.properties" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "server.properties" }));
    await waitFor(() => expect(screen.getByLabelText(/Editing server.properties/)).toBeTruthy());

    // A clean close goes straight back to the listing — no interrogation.
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "server.properties" })).toBeTruthy());
    expect(screen.queryByRole("dialog")).toBeNull();

    // A dirty close asks; cancel keeps the editor and the draft.
    fireEvent.click(screen.getByRole("button", { name: "server.properties" }));
    await waitFor(() => expect(screen.getByLabelText(/Editing server.properties/)).toBeTruthy());
    fireEvent.change(screen.getByLabelText(/Editing server.properties/), { target: { value: "motd=changed\n" } });
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.getByRole("dialog")).toBeTruthy());
    expect(screen.getByRole("dialog").textContent).toContain("unsaved changes");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByLabelText(/Editing server.properties/)).toBeTruthy();
    expect(screen.getByText("unsaved changes")).toBeTruthy();

    // Confirm discards: the listing returns, the draft is gone.
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.getByRole("dialog")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "server.properties" })).toBeTruthy());
    expect(screen.queryByLabelText(/Editing/)).toBeNull();
  });
});
