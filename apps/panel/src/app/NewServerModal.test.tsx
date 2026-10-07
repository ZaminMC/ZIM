// New Server flow: both doors. The download door walks the catalog
// (software → version → build), offers the Java picker with a fetch
// affordance, and drives the create job to completion; the register
// door keeps its client-side id sanity and structured error display.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  NewServerModal,
  formatProgressBytes,
  satisfyingRuntime,
  validateServerId,
} from "./NewServerModal";
import { useUi } from "../state/ui";
import { useJobs } from "../state/jobs";

vi.mock("../state/wire", () => ({ client: {}, startWire: vi.fn() }));
vi.mock("../state/actions", () => ({
  catalogList: vi.fn(),
  catalogVersions: vi.fn(),
  catalogBuilds: vi.fn(),
  createServer: vi.fn(),
  installJava: vi.fn(),
  listJava: vi.fn(),
  registerServer: vi.fn(),
  getServer: vi.fn(),
}));

import {
  catalogBuilds,
  catalogList,
  catalogVersions,
  createServer,
  getServer,
  installJava,
  listJava,
  registerServer,
} from "../state/actions";
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

describe("formatProgressBytes", () => {
  it("formats the three magnitudes users meet", () => {
    expect(formatProgressBytes(512)).toBe("512 B");
    expect(formatProgressBytes(64 * 1024)).toBe("64 KB");
    expect(formatProgressBytes(53 * 1024 * 1024)).toBe("53.0 MB");
  });
});

describe("satisfyingRuntime", () => {
  it("finds a runtime at or above the requirement", () => {
    const runtimes = [
      { path: "/a", major: 17, versionString: "17", vendor: "v", managed: false },
      { path: "/b", major: 21, versionString: "21", vendor: "v", managed: true },
    ];
    expect(satisfyingRuntime(runtimes, 21)?.path).toBe("/b");
    expect(satisfyingRuntime(runtimes, 25)).toBeUndefined();
    expect(satisfyingRuntime(runtimes, null)).toBeUndefined();
  });
});

describe("NewServerModal — download door", () => {
  beforeEach(() => {
    useUi.setState({ newServerOpen: true });
    useJobs.setState({ jobs: {} });
    const m = {
      catalogList: catalogList as unknown as ReturnType<typeof vi.fn>,
      catalogVersions: catalogVersions as unknown as ReturnType<typeof vi.fn>,
      catalogBuilds: catalogBuilds as unknown as ReturnType<typeof vi.fn>,
      createServer: createServer as unknown as ReturnType<typeof vi.fn>,
      installJava: installJava as unknown as ReturnType<typeof vi.fn>,
      listJava: listJava as unknown as ReturnType<typeof vi.fn>,
      registerServer: registerServer as unknown as ReturnType<typeof vi.fn>,
      getServer: getServer as unknown as ReturnType<typeof vi.fn>,
    };
    for (const mock of Object.values(m)) mock.mockReset();
    m.getServer.mockResolvedValue({
      serverId: "survival",
      displayName: "survival",
      state: "not-running",
    });
    m.catalogList.mockResolvedValue({
      entries: [
        { id: "paper", name: "Paper", description: "The default.", source: "fill" },
        {
          id: "fabric",
          name: "Fabric",
          description: "Lightweight mod loader.",
          source: "fabric-meta",
        },
      ],
    });
    m.catalogVersions.mockResolvedValue({
      project: "paper",
      versions: [{ id: "1.21.11" }, { id: "1.21.9" }],
    });
    m.catalogBuilds.mockResolvedValue({
      project: "paper",
      version: "1.21.11",
      javaMajor: 21,
      builds: [
        { id: 34, channel: "DEFAULT", download: { name: "p.jar", sha256: "x", url: "u" } },
      ],
    });
    m.listJava.mockResolvedValue({
      runtimes: [
        { path: "/jvm/21", major: 21, versionString: "21.0.3", vendor: "Temurin", managed: true },
      ],
    });
  });

  afterEach(cleanup);

  it("walks the catalog and creates through a job it watches to the end", async () => {
    (createServer as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      kind: "server.create",
      job: { jobId: "job-1", kind: "server.create", state: "running" },
    });
    render(<NewServerModal />);

    // Catalog pickers load and settle on their defaults.
    await waitFor(() =>
      expect(screen.getByLabelText<HTMLSelectElement>(/software/i).value).toBe("paper"),
    );
    await waitFor(() =>
      expect(screen.getByLabelText<HTMLSelectElement>(/^version/i).value).toBe("1.21.11"),
    );
    await waitFor(() =>
      expect(screen.getByLabelText<HTMLSelectElement>(/build/i).value).toBe("34"),
    );

    fireEvent.change(screen.getByLabelText(/^server id/i), { target: { value: "survival" } });
    fireEvent.click(screen.getByRole("button", { name: /download & create/i }));

    await waitFor(() => expect(createServer).toHaveBeenCalled());
    expect(createServer).toHaveBeenCalledWith(
      expect.objectContaining({
        serverId: "survival",
        project: "paper",
        version: "1.21.11",
        build: 34,
        javaPath: undefined,
      }),
    );

    // The modal watches the job; completion opens the new server.
    await waitFor(() => expect(useJobs.getState().jobs["job-1"]).toBeDefined());
    useJobs.getState().completed("job-1", "succeeded");
    await waitFor(() => expect(useUi.getState().newServerOpen).toBe(false));
    expect(useUi.getState().activeTab).toBe("survival");
  });

  it("creates a fabric server by pinning a loader instead of a build", async () => {
    (catalogVersions as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      project: "fabric",
      versions: [{ id: "1.21.11" }, { id: "1.20.1" }],
    });
    (catalogBuilds as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      project: "fabric",
      version: "1.21.11",
      javaMajor: 21,
      builds: [],
      loaders: ["0.16.14", "0.16.13"],
    });
    (createServer as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      kind: "server.create",
      job: { jobId: "fabric-1", kind: "server.create", state: "running" },
    });
    render(<NewServerModal />);

    await waitFor(() =>
      expect(screen.getByLabelText<HTMLSelectElement>(/software/i).value).toBe("paper"),
    );
    fireEvent.change(screen.getByLabelText(/software/i), { target: { value: "fabric" } });
    await waitFor(() =>
      expect(screen.getByLabelText<HTMLSelectElement>(/^version/i).value).toBe("1.21.11"),
    );

    // The loader select takes the build select's place, newest stable
    // loader preselected.
    expect(screen.queryByLabelText(/build/i)).toBeNull();
    await waitFor(() =>
      expect(screen.getByLabelText<HTMLSelectElement>(/loader/i).value).toBe("0.16.14"),
    );

    fireEvent.change(screen.getByLabelText(/^server id/i), { target: { value: "mods" } });
    fireEvent.click(screen.getByRole("button", { name: /download & create/i }));
    await waitFor(() => expect(createServer).toHaveBeenCalled());
    expect(createServer).toHaveBeenCalledWith(
      expect.objectContaining({
        serverId: "mods",
        project: "fabric",
        version: "1.21.11",
        build: undefined,
        loader: "0.16.14",
      }),
    );
  });

  it("offers the Java fetch when no runtime satisfies the requirement", async () => {
    (listJava as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      runtimes: [
        { path: "/jvm/17", major: 17, versionString: "17.0.2", vendor: "Temurin", managed: false },
      ],
    });
    (installJava as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      kind: "java.install",
      job: { jobId: "java-1", kind: "java.install", state: "running" },
    });
    render(<NewServerModal />);

    await screen.findByText(/No discovered runtime satisfies Java 21/i);
    fireEvent.click(screen.getByRole("button", { name: /Fetch Java 21/i }));
    await waitFor(() => expect(installJava).toHaveBeenCalledWith(21));
  });

  it("surfaces a typed creation failure and stays open", async () => {
    (createServer as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      kind: "server.create",
      job: { jobId: "job-2", kind: "server.create", state: "running" },
    });
    render(<NewServerModal />);
    await waitFor(() =>
      expect(screen.getByLabelText<HTMLSelectElement>(/^version/i).value).toBe("1.21.11"),
    );
    fireEvent.change(screen.getByLabelText(/^server id/i), { target: { value: "doomed" } });
    fireEvent.click(screen.getByRole("button", { name: /download & create/i }));

    await waitFor(() => expect(useJobs.getState().jobs["job-2"]).toBeDefined());
    useJobs.getState().completed("job-2", "failed", {
      code: "CHECKSUM_MISMATCH",
      message: "The downloaded file does not match its published checksum.",
      remediation: ["retry_download"],
    });
    await screen.findByRole("alert");
    expect(screen.getByText(/CHECKSUM_MISMATCH/)).toBeTruthy();
    expect(useUi.getState().newServerOpen).toBe(true);
  });

  it("blocks submit on a client-side id error", async () => {
    render(<NewServerModal />);
    await waitFor(() =>
      expect(screen.getByLabelText<HTMLSelectElement>(/^version/i).value).toBe("1.21.11"),
    );
    fireEvent.change(screen.getByLabelText(/^server id/i), { target: { value: "BAD ID" } });
    fireEvent.click(screen.getByRole("button", { name: /download & create/i }));
    expect(createServer).not.toHaveBeenCalled();
    expect(screen.getByText(/lowercase letters/)).toBeTruthy();
  });
});

describe("NewServerModal — register door", () => {
  beforeEach(() => {
    useUi.setState({ newServerOpen: true });
    (registerServer as unknown as ReturnType<typeof vi.fn>).mockReset();
  });

  afterEach(cleanup);

  it("registers, upserts, opens the tab, and closes", async () => {
    (registerServer as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      server: { serverId: "survival", displayName: "Survival", state: "not-running" },
    });
    render(<NewServerModal />);

    fireEvent.click(screen.getByRole("tab", { name: /register existing/i }));
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
    (registerServer as unknown as ReturnType<typeof vi.fn>).mockRejectedValue(
      new ProtocolRequestError({
        code: "SERVER_ID_EXISTS",
        message: "A server with this id is already registered.",
        remediation: ["Pick another id."],
      }),
    );
    render(<NewServerModal />);
    fireEvent.click(screen.getByRole("tab", { name: /register existing/i }));

    fireEvent.change(screen.getByLabelText(/server id/i), { target: { value: "dupe" } });
    fireEvent.change(screen.getByLabelText(/root directory/i), { target: { value: "/srv/mc" } });
    fireEvent.click(screen.getByRole("button", { name: "Register" }));

    await screen.findByRole("alert");
    expect(screen.getByText(/already registered/)).toBeTruthy();
    expect(screen.getByText("Pick another id.")).toBeTruthy();
    expect(useUi.getState().newServerOpen).toBe(true); // modal stays open
  });
});
