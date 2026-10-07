// Plugins: the catalog answers, the install button becomes a job, and
// the inventory is the directory's truth. The daemon side is e2e-tested
// against a real mock catalog (crates/zamind/tests/plugins.rs); here the
// view's contract holds: search renders hits, install hands the job to
// the jobs store, completion refreshes the inventory, delete confirms.

import { fireEvent, render, screen, cleanup, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PluginsView } from "./PluginsView";
import { ProtocolRequestError } from "../protocol/client";
import { useJobs } from "../state/jobs";

const mocks = vi.hoisted(() => ({
  pluginsSearch: vi.fn(),
  pluginsInstalled: vi.fn(),
  pluginsInstall: vi.fn(),
  pluginsDelete: vi.fn(),
  pluginsUpdates: vi.fn(),
}));

vi.mock("../state/actions", () => ({
  pluginsSearch: mocks.pluginsSearch,
  pluginsInstalled: mocks.pluginsInstalled,
  pluginsInstall: mocks.pluginsInstall,
  pluginsDelete: mocks.pluginsDelete,
  pluginsUpdates: mocks.pluginsUpdates,
}));

beforeEach(() => {
  useJobs.setState({ jobs: {} });
  mocks.pluginsSearch.mockReset();
  mocks.pluginsInstalled.mockReset().mockResolvedValue({
    target: "plugins",
    entries: [],
  });
  mocks.pluginsInstall.mockReset();
  mocks.pluginsDelete.mockReset();
  mocks.pluginsUpdates.mockReset();
});

afterEach(cleanup);

describe("PluginsView", () => {
  it("shows the daemon's target and the empty-catalog note", async () => {
    render(<PluginsView serverId="alpha" />);
    await waitFor(() =>
      expect(screen.getByText(/Installs land in plugins\//)).toBeTruthy(),
    );
    expect(
      screen.getByText(/Search the catalog to install a plugin/),
    ).toBeTruthy();
  });

  it("renders search hits with install buttons", async () => {
    mocks.pluginsSearch.mockResolvedValue({
      target: "plugins",
      hits: [
        {
          projectId: "AABBCC",
          slug: "essentialsx",
          title: "EssentialsX",
          description: "The essential plugin suite.",
          downloads: 4000000,
          loaders: ["paper", "spigot"],
        },
      ],
    });
    render(<PluginsView serverId="alpha" />);
    fireEvent.change(screen.getByLabelText(/Search the plugin catalog/), {
      target: { value: "essentials" },
    });
    fireEvent.submit(screen.getByRole("button", { name: /Search/ }));

    expect(await screen.findByText("EssentialsX")).toBeTruthy();
    expect(screen.getByText(/4,000,000 downloads · paper, spigot/)).toBeTruthy();
    const install = screen.getByRole<HTMLButtonElement>("button", { name: "Install" });
    expect(install.disabled).toBe(false);
  });

  it("an install hands the job to the store and the progress chip shows", async () => {
    mocks.pluginsSearch.mockResolvedValue({
      target: "plugins",
      hits: [
        {
          projectId: "AABBCC",
          slug: "essentialsx",
          title: "EssentialsX",
          description: "Suite.",
          downloads: 1,
          loaders: ["paper"],
        },
      ],
    });
    mocks.pluginsInstall.mockResolvedValue({
      kind: "plugins.install",
      job: {
        jobId: "job-1",
        kind: "plugins.install",
        serverId: "alpha",
        state: "running",
        createdAtMs: 1,
      },
    });
    render(<PluginsView serverId="alpha" />);
    fireEvent.change(screen.getByLabelText(/Search the plugin catalog/), {
      target: { value: "essentials" },
    });
    fireEvent.submit(screen.getByRole("button", { name: /Search/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Install" }));

    await waitFor(() =>
      expect(mocks.pluginsInstall).toHaveBeenCalledWith(
        "alpha",
        "AABBCC",
        undefined,
        false,
        undefined,
      ),
    );
    expect(mocks.pluginsInstall).toHaveBeenCalledTimes(1);

    // The daemon broadcasts job events on the wire; the store is the
    // panel's reconciled truth. Progress arrives; the chip shows it.
    useJobs.getState().started({
      jobId: "job-1",
      kind: "plugins.install",
      serverId: "alpha",
      state: "running",
      createdAtMs: 1,
      progress: { current: 640, total: 1024, unit: "bytes", message: "installing EssentialsX-2.20.0.jar" },
    });
    expect(await screen.findByRole("status")).toBeTruthy();
    expect(screen.getByText("installing EssentialsX-2.20.0.jar")).toBeTruthy();

    // While the job runs, installs stay disabled (one at a time).
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Install" }).disabled,
    ).toBe(true);

    // Completion releases the busy state and refreshes the inventory.
    useJobs.getState().completed("job-1", "succeeded");
    mocks.pluginsInstalled.mockResolvedValueOnce({
      target: "plugins",
      entries: [
        {
          fileName: "EssentialsX-2.20.0.jar",
          sizeBytes: 1024,
          modifiedMs: 1,
          symlinkOutside: false,
        },
      ],
    });
    await waitFor(() =>
      expect(screen.getByText("EssentialsX-2.20.0.jar")).toBeTruthy(),
    );
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Install" }).disabled,
    ).toBe(false);
  });

  it("a typed install rejection surfaces as an alert, not a stuck button", async () => {
    mocks.pluginsSearch.mockResolvedValue({
      target: "plugins",
      hits: [
        {
          projectId: "GHOST",
          slug: "ghost",
          title: "Ghost Plugin",
          description: "Nothing installable.",
          downloads: 0,
          loaders: ["paper"],
        },
      ],
    });
    mocks.pluginsInstall.mockRejectedValue(
      new Error("No installable plugins version found for this project."),
    );
    render(<PluginsView serverId="alpha" />);
    fireEvent.change(screen.getByLabelText(/Search the plugin catalog/), {
      target: { value: "ghost" },
    });
    fireEvent.submit(screen.getByRole("button", { name: /Search/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Install" }));

    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(screen.getByText(/No installable plugins version/)).toBeTruthy();
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Install" }).disabled,
    ).toBe(false);
  });

  it("a PLUGIN_EXISTS rejection offers the explicit replace", async () => {
    mocks.pluginsSearch.mockResolvedValue({
      target: "plugins",
      hits: [
        {
          projectId: "AABBCC",
          slug: "essentialsx",
          title: "EssentialsX",
          description: "Suite.",
          downloads: 1,
          loaders: ["paper"],
        },
      ],
    });
    mocks.pluginsInstall
      .mockRejectedValueOnce(
        new ProtocolRequestError({
          code: "PLUGIN_EXISTS",
          message:
            "The file \"EssentialsX-2.20.0.jar\" is already installed with different content; an update must replace it explicitly.",
          context: { file: "EssentialsX-2.20.0.jar" },
        }),
      )
      .mockResolvedValue({
        kind: "plugins.install",
        job: {
          jobId: "job-9",
          kind: "plugins.install",
          serverId: "alpha",
          state: "running",
          createdAtMs: 1,
        },
      });
    render(<PluginsView serverId="alpha" />);
    fireEvent.change(screen.getByLabelText(/Search the plugin catalog/), {
      target: { value: "essentials" },
    });
    fireEvent.submit(screen.getByRole("button", { name: /Search/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Install" }));

    // The typed refusal becomes the operator's decision, with the file named.
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(
      screen.getByText(/EssentialsX-2\.20\.0\.jar is already installed with different/),
    ).toBeTruthy();

    // Replace retries the install with the explicit overwrite.
    fireEvent.click(screen.getByRole("button", { name: "Replace" }));
    await waitFor(() =>
      expect(mocks.pluginsInstall).toHaveBeenCalledWith(
        "alpha",
        "AABBCC",
        undefined,
        true,
        undefined,
      ),
    );

    // The retried install hands its job to the store like any other.
    useJobs.getState().started({
      jobId: "job-9",
      kind: "plugins.install",
      serverId: "alpha",
      state: "running",
      createdAtMs: 1,
      progress: { current: 10, total: 100, unit: "bytes", message: "installing EssentialsX-2.20.0.jar" },
    });
    expect(await screen.findByRole("status")).toBeTruthy();
  });

  it("dismiss the replace offer and nothing is overwritten", async () => {
    mocks.pluginsSearch.mockResolvedValue({
      target: "plugins",
      hits: [
        {
          projectId: "AABBCC",
          slug: "essentialsx",
          title: "EssentialsX",
          description: "Suite.",
          downloads: 1,
          loaders: ["paper"],
        },
      ],
    });
    mocks.pluginsInstall.mockRejectedValueOnce(
      new ProtocolRequestError({
        code: "PLUGIN_EXISTS",
        message: "already installed with different content",
        context: { file: "EssentialsX-2.20.0.jar" },
      }),
    );
    render(<PluginsView serverId="alpha" />);
    fireEvent.change(screen.getByLabelText(/Search the plugin catalog/), {
      target: { value: "essentials" },
    });
    fireEvent.submit(screen.getByRole("button", { name: /Search/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Install" }));
    await screen.findByRole("alert");

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("button", { name: "Replace" })).toBeNull();
    expect(mocks.pluginsInstall).toHaveBeenCalledTimes(1);
  });

  it("the update check rides verdict chips on the installed rows", async () => {
    mocks.pluginsInstalled.mockResolvedValue({
      target: "plugins",
      entries: [
        { fileName: "EssentialsX-2.19.0.jar", sizeBytes: 10, modifiedMs: 0, symlinkOutside: false },
        { fileName: "hand-dropped.jar", sizeBytes: 10, modifiedMs: 0, symlinkOutside: false },
      ],
    });
    mocks.pluginsUpdates.mockResolvedValue({
      target: "plugins",
      entries: [
        {
          fileName: "EssentialsX-2.19.0.jar",
          status: "update-available",
          projectId: "AABBCC",
          installedVersion: "2.19.0",
          latestVersion: "2.20.0",
          latestVersionId: "ver9",
        },
        { fileName: "hand-dropped.jar", status: "unmanaged" },
      ],
    });

    // The recipe's install hands off to the jobs store like any other.
    mocks.pluginsInstall.mockResolvedValue({});
    render(<PluginsView serverId="alpha" />);
    fireEvent.click(await screen.findByRole("button", { name: "Check updates" }));

    // The verdicts land as chips on the rows they describe, with the
    // newest version named where one exists.
    expect(
      await screen.findByText("update: 2.20.0"),
    ).toBeTruthy();
    expect(screen.getByText("unmanaged")).toBeTruthy();

    // The update button is the recipe applied: same server, the pin,
    // the explicit replace the rule requires — and the retire step,
    // naming the row's own jar so the update does not leave both
    // versions on disk (the overwrite rule is name-keyed).
    fireEvent.click(screen.getByRole("button", { name: "update" }));
    await waitFor(() =>
      expect(mocks.pluginsInstall).toHaveBeenCalledWith(
        "alpha",
        "AABBCC",
        "ver9",
        true,
        "EssentialsX-2.19.0.jar",
      ),
    );
  });

  it("an up-to-date verdict shows without an update button", async () => {
    mocks.pluginsInstalled.mockResolvedValue({
      target: "plugins",
      entries: [
        { fileName: "EssentialsX-2.20.0.jar", sizeBytes: 10, modifiedMs: 0, symlinkOutside: false },
      ],
    });
    mocks.pluginsUpdates.mockResolvedValue({
      target: "plugins",
      entries: [
        {
          fileName: "EssentialsX-2.20.0.jar",
          status: "up-to-date",
          projectId: "AABBCC",
          installedVersion: "2.20.0",
          latestVersion: "2.20.0",
          latestVersionId: "ver9",
        },
      ],
    });

    render(<PluginsView serverId="alpha" />);
    fireEvent.click(await screen.findByRole("button", { name: "Check updates" }));

    expect(await screen.findByText("up to date")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "update" })).toBeNull();
    // The plain remove still works — the row keeps its own affordance.
    expect(screen.getByRole("button", { name: "remove" })).toBeTruthy();
  });
});
