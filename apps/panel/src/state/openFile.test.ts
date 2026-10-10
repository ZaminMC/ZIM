// The §4 handoff's contract: what ZIM may conclude about an OS-handed
// path, and nothing more. The verdict comes from the daemon's own
// discovery answer — the double-click never invents a server, never
// annexes a folder, and never opens the same offer twice (the claim is
// the host's; the plan is what gets tested here).

import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  normalizeOsPath,
  planJarOpen,
  serverIdFromDirectoryName,
} from "./openFile";
import type { DiscoveredServer } from "../protocol/types";

vi.mock("./actions", () => ({
  discoverServers: vi.fn(),
  listServers: vi.fn(),
  registerServer: vi.fn(),
}));

vi.mock("../integration/tauri", () => ({
  invokeHost: vi.fn(),
  listenHost: vi.fn(async () => () => {}),
}));

import { discoverServers, listServers } from "./actions";
import { invokeHost } from "../integration/tauri";
import { openServerFile } from "./openFile";
import { useServers } from "./servers";

const discoverMock = discoverServers as ReturnType<typeof vi.fn>;
const listMock = listServers as ReturnType<typeof vi.fn>;
const invokeMock = invokeHost as ReturnType<typeof vi.fn>;

const JAR = "/srv/smp/paper-26.2-129.jar";
const ROOT = "/srv/smp";

function discovered(kind: DiscoveredServer["kind"], path: string, extra: Partial<DiscoveredServer> = {}): DiscoveredServer {
  return { kind, path, ...extra };
}

beforeEach(() => {
  discoverMock.mockReset();
  listMock.mockReset();
  invokeMock.mockReset().mockResolvedValue(undefined);
  useServers.setState({ servers: {}, crashes: {} });
});

describe("normalizeOsPath", () => {
  it("strips the shell association's quoting habits", () => {
    expect(normalizeOsPath(`  "${JAR}" `)).toBe(JAR);
    expect(normalizeOsPath(JAR)).toBe(JAR);
  });
});

describe("planJarOpen", () => {
  const registered = new Set(["smp"]);

  it("opens the server that already owns the directory", () => {
    const plan = planJarOpen(
      JAR,
      [discovered("registered", ROOT, { serverId: "smp" })],
      registered,
    );
    expect(plan).toEqual({ kind: "open", serverId: "smp" });
  });

  it("opens the marker's owner when the row came back unregistered", () => {
    const plan = planJarOpen(
      JAR,
      [discovered("directory", ROOT, { marker: "smp" })],
      registered,
    );
    expect(plan).toEqual({ kind: "open", serverId: "smp" });
  });

  it("registers a server-shaped directory nobody owns, id from its name", () => {
    const plan = planJarOpen(JAR, [discovered("directory", ROOT)], new Set());
    expect(plan).toEqual({
      kind: "register",
      serverId: "smp",
      displayName: "smp",
      rootPath: ROOT,
    });
  });

  it("never annexes a folder for a standalone jar — the discovery page answers", () => {
    // The jar is classified (a real server family), but its parent is not
    // a server directory: Downloads must not become a server root.
    const plan = planJarOpen(
      "/home/op/Downloads/paper-26.2-129.jar",
      [discovered("jar", "/home/op/Downloads/paper-26.2-129.jar", { platform: "paper" })],
      new Set(),
    );
    expect(plan).toEqual({ kind: "discover", query: "/home/op/Downloads" });
  });

  it("a jar discovery never classified lands on the same honest page", () => {
    const plan = planJarOpen("/home/op/Downloads/mod.jar", [], new Set());
    expect(plan).toEqual({ kind: "discover", query: "/home/op/Downloads" });
  });

  it("a non-jar handoff is the folder's discovery page, nothing more", () => {
    const plan = planJarOpen("/srv/smp/server.properties", [], new Set());
    expect(plan).toEqual({ kind: "discover", query: "/srv/smp" });
  });

  it("matches paths across the OS's spelling moods (case, separators)", () => {
    const plan = planJarOpen(
      "C:\\Servers\\SMP\\Paper-26.2-129.JAR",
      [discovered("registered", "c:/servers/smp", { serverId: "smp" })],
      registered,
    );
    expect(plan).toEqual({ kind: "open", serverId: "smp" });
  });

  it("a duplicate registration of the same root opens it instead", () => {
    // The daemon's idempotency story from the client side: the same
    // directory double-clicked after registration answers "open", so no
    // second server ever appears for one root.
    const first = planJarOpen(JAR, [discovered("directory", ROOT)], new Set());
    expect(first.kind).toBe("register");
    const again = planJarOpen(
      JAR,
      [discovered("registered", ROOT, { serverId: "smp" })],
      new Set(["smp"]),
    );
    expect(again).toEqual({ kind: "open", serverId: "smp" });
  });
});

describe("serverIdFromDirectoryName", () => {
  it("slugs the directory's own name into a legal id", () => {
    expect(serverIdFromDirectoryName("My Survival Server!", new Set())).toBe("my-survival-server");
    expect(serverIdFromDirectoryName("SMP", new Set())).toBe("smp");
  });

  it("falls back to `server` when nothing survives the slug", () => {
    expect(serverIdFromDirectoryName("..", new Set())).toBe("server");
    expect(serverIdFromDirectoryName("!!", new Set())).toBe("server");
  });

  it("uniquifies against the live registry, never inventing a collision", () => {
    expect(serverIdFromDirectoryName("smp", new Set(["smp"]))).toBe("smp-2");
    expect(serverIdFromDirectoryName("smp", new Set(["smp", "smp-2"]))).toBe("smp-3");
  });
});

describe("openServerFile", () => {
  it("opens an owned directory's server in a new tab", async () => {
    discoverMock.mockResolvedValue({
      servers: [discovered("registered", ROOT, { serverId: "smp" })],
    });
    listMock.mockResolvedValue({
      servers: [{ serverId: "smp", displayName: "SMP", state: "stopped" }],
    });

    await openServerFile(JAR);

    expect(invokeMock).toHaveBeenCalledWith("shell_command", {
      id: 34014,
      arg: { destination: { kind: "server", serverId: "smp" } },
    });
  });

  it("registers an unowned server-shaped directory, seeds the store, opens the tab", async () => {
    discoverMock.mockResolvedValue({ servers: [discovered("directory", ROOT)] });
    listMock.mockResolvedValue({ servers: [] });
    const { registerServer } = await import("./actions");
    vi.mocked(registerServer).mockResolvedValue({
      server: { serverId: "smp", displayName: "smp", state: "not-running" },
    });

    await openServerFile(JAR);

    expect(vi.mocked(registerServer)).toHaveBeenCalledWith({
      serverId: "smp",
      displayName: "smp",
      rootPath: ROOT,
    });
    // The store seed: the tab's first paint is true even if the event
    // races the reply.
    expect(useServers.getState().servers["smp"]?.state).toBe("not-running");
    expect(invokeMock).toHaveBeenCalledWith("shell_command", {
      id: 34014,
      arg: { destination: { kind: "server", serverId: "smp" } },
    });
  });

  it("a standalone jar lands on the new-tab page with the folder as its query", async () => {
    discoverMock.mockResolvedValue({
      servers: [discovered("jar", "/home/op/Downloads/paper-26.2-129.jar")],
    });
    listMock.mockResolvedValue({ servers: [] });

    await openServerFile("/home/op/Downloads/paper-26.2-129.jar");

    expect(invokeMock).toHaveBeenCalledWith("shell_command", {
      id: 34014,
      arg: {
        destination: { kind: "new" },
        query: "/home/op/Downloads",
      },
    });
  });

  it("a dead wire falls back to the same honest page", async () => {
    discoverMock.mockRejectedValue(new Error("wire down"));
    listMock.mockRejectedValue(new Error("wire down"));

    await openServerFile(JAR);

    expect(invokeMock).toHaveBeenCalledWith("shell_command", {
      id: 34014,
      arg: { destination: { kind: "new" }, query: ROOT },
    });
  });
});
