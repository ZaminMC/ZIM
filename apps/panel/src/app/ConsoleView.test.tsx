// The structured console (ADR-0020): §29 filters are render-time views over
// an intact buffer, §28's copy control copies by mode with its tint and its
// contextual menu, §26's composer rides the ordinary stdin path, and §27's
// icon hands the console to a dedicated tab exactly once per click.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConsoleView, copyTextFor } from "./ConsoleView";
import { useServers } from "../state/servers";
import type { LogLine, LogRangeResult } from "../protocol/types";

const mocks = vi.hoisted(() => ({
  sendStdin: vi.fn(),
  clipboardWrite: vi.fn(),
}));

vi.mock("../state/actions", () => ({
  sendStdin: mocks.sendStdin,
  rangeLogs: vi.fn(),
  subscribeLogs: vi.fn(),
}));

import { rangeLogs, subscribeLogs } from "../state/actions";

const rangeLogsMock = vi.mocked(rangeLogs);
const subscribeLogsMock = vi.mocked(subscribeLogs);
const sendStdinMock = mocks.sendStdin;

function page(startOffset: number, lines: string[], olderAvailable = true): LogRangeResult {
  return {
    file: "logs/latest.log",
    lines: lines.map((text) => ({ tsMs: 0, level: "info", thread: "Server thread", line: text })),
    olderAvailable,
    startOffset,
  };
}

function line(level: LogLine["level"], text: string, thread = "Server thread"): LogLine {
  return { tsMs: 0, level, thread, line: text };
}

function seedRegistry(state: "running" | "stopped"): void {
  useServers.setState({
    servers: {
      alpha: {
        serverId: "alpha",
        displayName: "Alpha",
        state,
      },
    },
  } as unknown as ReturnType<typeof useServers.getState>["servers"]);
}

beforeEach(() => {
  sendStdinMock.mockReset();
  sendStdinMock.mockResolvedValue(undefined);
  mocks.clipboardWrite.mockReset();
  mocks.clipboardWrite.mockResolvedValue(undefined);
  rangeLogsMock.mockReset();
  subscribeLogsMock.mockReset();
  subscribeLogsMock.mockImplementation(() =>
    Promise.resolve({ dispose: () => {}, result: null }),
  );
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText: mocks.clipboardWrite },
    configurable: true,
    writable: true,
  });
});

afterEach(() => {
  cleanup();
  useServers.setState({ servers: {} } as unknown as ReturnType<typeof useServers.getState>["servers"]);
});

describe("console filters (§29)", () => {
  it("filters the view without destroying the underlying stream", async () => {
    rangeLogsMock.mockResolvedValue({
      file: "logs/latest.log",
      lines: [
        line("info", "starting minecraft"),
        line("warn", "watchdog warning"),
        line("error", "fatal bound exception"),
      ],
      olderAvailable: true,
      startOffset: 4096,
    });
    seedRegistry("running");
    render(<ConsoleView serverId="alpha" />);
    await waitFor(() => screen.getByText("starting minecraft"));

    // The founder's row: Show all | Info | Warnings | Errors (+ Debug).
    expect(screen.getByRole("button", { name: "Show all" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Warnings" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Errors" }));
    expect(screen.queryByText("starting minecraft")).toBeNull();
    expect(screen.getByText(/fatal bound exception/)).toBeTruthy();

    // Back to all: nothing was lost — the buffer was never touched.
    fireEvent.click(screen.getByRole("button", { name: "Show all" }));
    expect(screen.getByText(/starting minecraft/)).toBeTruthy();
    expect(screen.getByText(/watchdog warning/)).toBeTruthy();
  });

  it("says the stream is untouched when a filter matches nothing", async () => {
    rangeLogsMock.mockResolvedValue(page(0, ["only info here"], false));
    seedRegistry("running");
    render(<ConsoleView serverId="alpha" />);
    await waitFor(() => screen.getByText("only info here"));

    fireEvent.click(screen.getByRole("button", { name: "Errors" }));
    expect(
      screen.getByText(/No lines match the current filter — the stream itself is untouched/),
    ).toBeTruthy();
  });
});

describe("copy control (§28)", () => {
  it("copies by mode; the context menu changes the mode", async () => {
    rangeLogsMock.mockResolvedValue({
      file: "logs/latest.log",
      lines: [line("info", "done line"), line("warn", "warn line"), line("error", "error line")],
      olderAvailable: false,
      startOffset: 0,
    });
    seedRegistry("running");
    render(<ConsoleView serverId="alpha" />);
    await waitFor(() => screen.getByText("done line"));

    // Default mode is All: the click copies the loaded view.
    const copy = screen.getByRole("button", { name: /copy all/i });
    fireEvent.click(copy);
    await waitFor(() => expect(mocks.clipboardWrite).toHaveBeenCalledTimes(1));
    const all = mocks.clipboardWrite.mock.calls[0]?.[0] as string;
    expect(all).toContain("done line");
    expect(all).toContain("warn line");
    expect(all).toContain("error line");

    // Right-click opens the contextual menu — no modal (§28).
    fireEvent.contextMenu(copy);
    const errorsItem = await screen.findByRole("menuitemradio", { name: "All errors" });
    fireEvent.click(errorsItem);
    expect(screen.queryByRole("menu")).toBeNull();

    // The button now speaks errors, tinted red.
    const errorCopy = screen.getByRole("button", { name: /copy all errors/i });
    fireEvent.click(errorCopy);
    await waitFor(() => expect(mocks.clipboardWrite).toHaveBeenCalledTimes(2));
    const errors = mocks.clipboardWrite.mock.calls[1]?.[0] as string;
    expect(errors).toContain("error line");
    expect(errors).not.toContain("warn line");
  });

  it("formats copied lines as [level] [thread] message", () => {
    const text = copyTextFor("all", [
      line("warn", "careful", "Worker"),
      line("info", "plain", ""),
    ]);
    expect(text).toBe("[warn] [Worker] careful\n[info] plain");
  });

  it("reports an empty mode honestly instead of pretending", async () => {
    rangeLogsMock.mockResolvedValue(page(0, ["info only"], false));
    seedRegistry("running");
    render(<ConsoleView serverId="alpha" />);
    await waitFor(() => screen.getByText("info only"));

    fireEvent.contextMenu(screen.getByRole("button", { name: /copy all/i }));
    fireEvent.click(await screen.findByRole("menuitemradio", { name: "All errors" }));
    fireEvent.click(screen.getByRole("button", { name: /copy all errors/i }));
    await waitFor(() =>
      expect(screen.getByText("no error lines in the loaded view")).toBeTruthy(),
    );
  });
});

describe("command composer (§26)", () => {
  it("sends the typed line over stdin and clears itself", async () => {
    rangeLogsMock.mockResolvedValue(page(0, ["ready"], false));
    seedRegistry("running");
    render(<ConsoleView serverId="alpha" />);
    await waitFor(() => screen.getByText("ready"));

    const input = screen.getByLabelText<HTMLInputElement>("Minecraft command");
    fireEvent.change(input, { target: { value: "say hello" } });
    fireEvent.submit(input.closest("form") as HTMLFormElement);

    await waitFor(() => expect(sendStdinMock).toHaveBeenCalledWith("alpha", "say hello"));
    await waitFor(() => expect(input.value).toBe(""));
  });

  it("refuses honestly while the server is not running", async () => {
    rangeLogsMock.mockResolvedValue(page(0, [], false));
    seedRegistry("stopped");
    render(<ConsoleView serverId="alpha" />);

    expect(screen.getByText("server is not running — commands are rejected")).toBeTruthy();
    expect(screen.getByLabelText<HTMLInputElement>("Minecraft command").disabled).toBe(true);
    expect(sendStdinMock).not.toHaveBeenCalled();
  });

  it("surfaces a stdin rejection instead of swallowing it", async () => {
    rangeLogsMock.mockResolvedValue(page(0, ["ready"], false));
    seedRegistry("running");
    sendStdinMock.mockRejectedValue(new Error("rejected"));
    render(<ConsoleView serverId="alpha" />);
    await waitFor(() => screen.getByText("ready"));

    const input = screen.getByLabelText("Minecraft command");
    fireEvent.change(input, { target: { value: "stop" } });
    fireEvent.submit(input.closest("form") as HTMLFormElement);

    expect(await screen.findByText("stdin rejected: is the server running?")).toBeTruthy();
  });
});

describe("dedicated tab (§27)", () => {
  it("offers the open-in-new-tab icon exactly where it makes sense", async () => {
    rangeLogsMock.mockResolvedValue(page(0, ["ready"], false));
    seedRegistry("running");
    const open = vi.fn();
    const { rerender } = render(<ConsoleView serverId="alpha" onOpenInNewTab={open} />);
    await waitFor(() => screen.getByText("ready"));

    fireEvent.click(screen.getByRole("button", { name: "Open console in new tab" }));
    expect(open).toHaveBeenCalledTimes(1);

    // The dedicated tab itself does not carry the icon.
    rerender(<ConsoleView serverId="alpha" variant="dedicated" />);
    expect(screen.queryByRole("button", { name: "Open console in new tab" })).toBeNull();
  });
});
