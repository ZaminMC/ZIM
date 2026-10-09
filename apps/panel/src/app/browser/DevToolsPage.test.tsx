// The developer tools' contract tests. zim://devtools is Part 2's control
// center and its security story is the page itself: the console speaks the
// protocol through a capability-named `zim` object — and the dangerous
// capabilities are NOT gated, they are ABSENT. These tests pin that law
// from the outside (through the console input, the way an operator would),
// plus the inspector's honest six-element facts, the servers section, and
// the live events feed.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DevToolsPage } from "./DevToolsPage";
import { useConnection } from "../../state/connection";
import { useServers } from "../../state/servers";
import type { CoreEvent } from "../../protocol/types";

// jsdom has no scrolling implementation; the console's auto-scroll rides
// Element.scrollTo, which is real browser behavior the tests must not break.
Element.prototype.scrollTo = () => {};

vi.mock("../../state/shellLane", () => ({
  navigateHost: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../state/wire", () => {
  let payloadSink: ((n: { payload: unknown }) => void) | null = null;
  return {
    client: {
      subscribe: vi.fn(
        (
          _topic: string,
          _arg: unknown,
          handlers: { onPayload: (n: { payload: unknown }) => void },
        ) => {
          payloadSink = handlers.onPayload;
          return Promise.resolve({ dispose: () => (payloadSink = null) });
        },
      ),
      // The events test pushes through this seam.
      __sink: () => payloadSink,
    },
  };
});

vi.mock("../../state/actions", () => ({
  getServerConfig: vi.fn(),
  startServer: vi.fn().mockResolvedValue(undefined),
  stopServer: vi.fn().mockResolvedValue(undefined),
}));

import { navigateHost } from "../../state/shellLane";
import { getServerConfig, startServer } from "../../state/actions";
import { client } from "../../state/wire";

const navigateMock = navigateHost as ReturnType<typeof vi.fn>;
const configMock = getServerConfig as ReturnType<typeof vi.fn>;
const startMock = startServer as ReturnType<typeof vi.fn>;
const subscribeMock = client.subscribe as ReturnType<typeof vi.fn>;

const SERVER = {
  serverId: "survival",
  displayName: "Survival",
  state: "running" as const,
  port: 25565,
};

const CONFIG = {
  serverId: "survival",
  displayName: "Survival",
  effective: {
    stopTimeoutSecs: 10,
    startupTimeoutSecs: 60,
    extraJvmArgs: [],
    backupKeep: 3,
    cpuPercent: 200,
    storageBytes: 50 * 1024 ** 3,
    sandboxMode: "auto",
    networkPolicy: "local-only",
  },
  provenance: { fields: {} },
};

function seedReady(): void {
  useConnection.setState({ status: "ready", daemon: { name: "zamind", version: "0" }, lastError: null });
  useServers.setState({ servers: { [SERVER.serverId]: SERVER }, crashes: {} });
}

async function renderReady() {
  seedReady();
  const view = render(<DevToolsPage />);
  await waitFor(() => expect(screen.getByRole("heading", { name: "Developer tools" })).toBeTruthy());
  return view;
}

async function openConsole() {
  await renderReady();
  const input = screen.getByRole("textbox", { name: "Developer console input" });
  return input;
}

async function runCode(input: HTMLElement, code: string) {
  fireEvent.change(input, { target: { value: code } });
  fireEvent.keyDown(input, { key: "Enter" });
}

function consoleLines(): string[] {
  return Array.from(document.querySelectorAll("div[class*='consoleLines'] > div")).map(
    (el) => el.textContent ?? "",
  );
}

beforeEach(() => {
  navigateMock.mockReset().mockResolvedValue(undefined);
  configMock.mockReset();
  startMock.mockReset().mockResolvedValue(undefined);
  subscribeMock.mockClear();
});

afterEach(() => {
  cleanup();
  useConnection.setState({ status: "offline", daemon: null, lastError: null });
  useServers.setState({ servers: {}, crashes: {} });
});

describe("DevToolsPage — the gate", () => {
  it("an unready daemon is a waiting page, never a half console", async () => {
    useConnection.setState({ status: "connecting" });
    render(<DevToolsPage />);
    expect(screen.getByText("Waiting for the daemon…")).toBeTruthy();
    expect(screen.queryByRole("textbox")).toBeNull();
  });
});

describe("DevToolsPage — the sections", () => {
  it("renders the four sections and switches between them", async () => {
    configMock.mockResolvedValue(CONFIG);
    await renderReady();
    for (const name of ["Console", "Servers", "Inspector", "Events"]) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }
    fireEvent.click(screen.getByRole("button", { name: "Servers" }));
    expect(screen.getByText(/survival · running · :25565/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Inspector" }));
    expect(screen.getByText(/sandbox record/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Console" }));
    expect(screen.getByRole("textbox", { name: "Developer console input" })).toBeTruthy();
  });
});

describe("DevToolsPage — the console and the zim capability object", () => {
  it("the capability surface is exactly servers, events, daemon — nothing else exists to reach", async () => {
    const input = await openConsole();
    await runCode(input, "Object.keys(zim).sort().join(',')");
    await waitFor(() => {
      expect(consoleLines().some((l) => l.includes("daemon,events,servers"))).toBe(true);
    });
  });

  it("the dangerous capabilities are absent, not gated", async () => {
    const input = await openConsole();
    await runCode(input, "String(zim.fs) + '|' + String(zim.process) + '|' + String(zim.net)");
    await waitFor(() => {
      // undefined everywhere — there is no fs/process/network primitive
      // on the object, gated or otherwise.
      expect(consoleLines().some((l) => l.includes("undefined|undefined|undefined"))).toBe(true);
    });
  });

  it("zim.servers.list() answers from the same store the UI uses", async () => {
    const input = await openConsole();
    await runCode(input, "zim.servers.list()");
    await waitFor(() => {
      const out = consoleLines().find((l) => l.includes("survival"));
      expect(out).toBeTruthy();
      expect(out).toContain("Survival");
      expect(out).toContain("running");
    });
  });

  it("zim.servers.start() goes through the protocol's action lane", async () => {
    const input = await openConsole();
    await runCode(input, 'zim.servers.start("survival")');
    await waitFor(() => {
      expect(startMock).toHaveBeenCalledWith("survival");
    });
  });

  it("a promise outcome resolves into an out line; a rejection becomes an error line", async () => {
    const input = await openConsole();
    await runCode(input, "Promise.resolve(42)");
    await waitFor(() => {
      expect(consoleLines().some((l) => l.includes("42"))).toBe(true);
    });
    // A different spelling so the input's own echo cannot satisfy the
    // assertion — the ERROR line itself must say the error's words.
    await runCode(input, "Promise.reject(new Error('the wire said no'))");
    await waitFor(() => {
      expect(consoleLines().some((l) => l === "Error: the wire said no")).toBe(true);
    });
  });

  it("a syntax error is an error line naming the error, never a crash and never \"{}\"", async () => {
    const input = await openConsole();
    await runCode(input, "this is not javascript ((");
    await waitFor(() => {
      expect(consoleLines().some((l) => l.startsWith("SyntaxError:"))).toBe(true);
    });
  });

  it("ArrowUp recalls the previous input, ArrowDown returns past it", async () => {
    const input = await openConsole();
    await runCode(input, "zim.daemon()");
    await runCode(input, "1 + 1");
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect((input as HTMLInputElement).value).toBe("1 + 1");
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect((input as HTMLInputElement).value).toBe("zim.daemon()");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect((input as HTMLInputElement).value).toBe("1 + 1");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect((input as HTMLInputElement).value).toBe("");
  });
});

describe("DevToolsPage — the inspector's honest facts", () => {
  it("renders the six-element boundary record with the server's own numbers", async () => {
    configMock.mockResolvedValue(CONFIG);
    await renderReady();
    fireEvent.click(screen.getByRole("button", { name: "Inspector" }));
    // The six elements: process sandbox, filesystem, memory, CPU,
    // processes, storage, network — each says what holds it.
    await waitFor(() => expect(screen.getByText("Process sandbox")).toBeTruthy());
    expect(screen.getByText(/AppContainer \+ Job Object/)).toBeTruthy();
    expect(screen.getByText(/Jailed to the server root/)).toBeTruthy();
    expect(screen.getByText(/Job-object cap/)).toBeTruthy();
    expect(screen.getByText("Hard scheduler cap at 200% of one core")).toBeTruthy();
    expect(screen.getByText(/Job ceiling 64/)).toBeTruthy();
    expect(screen.getByText(/Budget 50 GiB/)).toBeTruthy();
    // The honesty clause — accounted, not an OS quota.
    expect(screen.getByText(/not an OS quota/)).toBeTruthy();
    expect(screen.getByText(/loopback reachable/)).toBeTruthy();
  });

  it("an uncapped server says so and names where to set the ceiling", async () => {
    configMock.mockResolvedValue({
      ...CONFIG,
      effective: { ...CONFIG.effective, cpuPercent: undefined, storageBytes: undefined, networkPolicy: "blocked-outbound" },
    });
    await renderReady();
    fireEvent.click(screen.getByRole("button", { name: "Inspector" }));
    await waitFor(() => {
      expect(screen.getByText("Uncapped — set a ceiling on the Startup tab")).toBeTruthy();
    });
    expect(screen.getByText("Accounted, uncapped — set a budget on the Startup tab")).toBeTruthy();
    expect(screen.getByText(/All outbound refused by the OS/)).toBeTruthy();
  });

  it("a failed config fetch never blanks the panel", async () => {
    configMock.mockRejectedValue(new Error("the daemon is unreachable"));
    await renderReady();
    fireEvent.click(screen.getByRole("button", { name: "Inspector" }));
    await waitFor(() => expect(configMock).toHaveBeenCalled());
    // The page lives; the facts stay absent (no last-good record yet).
    expect(screen.getByText(/sandbox record/)).toBeTruthy();
    expect(screen.queryByText("Hard scheduler cap at 200% of one core")).toBeNull();
  });
});

describe("DevToolsPage — the live events feed", () => {
  it("subscribes while open and renders arriving events", async () => {
    await renderReady();
    fireEvent.click(screen.getByRole("button", { name: "Events" }));
    await waitFor(() => expect(subscribeMock).toHaveBeenCalledWith("events", undefined, expect.objectContaining({ onPayload: expect.any(Function) })));
    expect(screen.getByText(/Listening to the live events stream/)).toBeTruthy();
    // Push one event through the captured handler.
    const event = { kind: "serverStateChanged", serverId: "survival", state: "stopped" } as unknown as CoreEvent;
    const captured = subscribeMock.mock.calls.at(-1)?.[2] as { onPayload: (n: { payload: unknown }) => void };
    captured.onPayload({ payload: { kind: "event", event } });
    await waitFor(() => {
      expect(screen.getByText(/serverStateChanged/)).toBeTruthy();
    });
  });
});

describe("DevToolsPage — the servers section", () => {
  it("opens a server through the host navigation lane", async () => {
    await renderReady();
    fireEvent.click(screen.getByRole("button", { name: "Servers" }));
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({ kind: "server", serverId: "survival" });
    });
  });
});
