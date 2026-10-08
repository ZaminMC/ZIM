// The jobs page (§73, ADR-0026): the seed is the daemon's `jobs.list`
// answer (the reconnection-proof record), live events ride the jobs
// store, the cancel verb speaks `jobs.cancel` and waits for the daemon's
// own state flip, and every failure is a typed note — nothing invented.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { JobsPage } from "./JobsPage";
import { useJobs } from "../state/jobs";
import { useServers } from "../state/servers";
import { ProtocolRequestError } from "../protocol/client";
import type { Job } from "../protocol/types";

vi.mock("../state/actions", () => ({
  listJobs: vi.fn(),
  cancelJob: vi.fn(),
}));

import { cancelJob, listJobs } from "../state/actions";

const listJobsMock = listJobs as ReturnType<typeof vi.fn>;
const cancelJobMock = cancelJob as ReturnType<typeof vi.fn>;

function jobOf(id: string, extra: Partial<Job> = {}): Job {
  return {
    jobId: id,
    kind: "backup.create",
    serverId: "demo",
    state: "running",
    createdAtMs: 1,
    ...extra,
  };
}

beforeEach(() => {
  localStorage.clear();
  useJobs.setState({ jobs: {} });
  useServers.setState({ servers: { demo: { serverId: "demo", displayName: "Demo", state: "running" } } });
  listJobsMock.mockReset().mockResolvedValue({ jobs: [] });
  cancelJobMock.mockReset().mockResolvedValue(jobOf("job-1", { state: "cancelled" }));
});

afterEach(cleanup);

describe("JobsPage", () => {
  it("seeds from the daemon's record and renders the rows newest-first", async () => {
    listJobsMock.mockResolvedValue({
      jobs: [
        jobOf("older", { createdAtMs: 100 }),
        jobOf("newer", { createdAtMs: 200, kind: "backup.restore", state: "queued" }),
      ],
    });
    render(<JobsPage />);
    await waitFor(() => expect(listJobsMock).toHaveBeenCalled());
    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]!.textContent).toContain("backup.restore");
    expect(rows[0]!.textContent).toContain("Queued");
    expect(rows[1]!.textContent).toContain("backup.create");
    expect(rows[1]!.textContent).toContain("Demo");
    // The snapshot landed in the store, so live events keep building on it.
    expect(Object.keys(useJobs.getState().jobs)).toEqual(["older", "newer"]);
  });

  it("renders live progress with a meter and the honest fraction", async () => {
    listJobsMock.mockResolvedValue({
      jobs: [jobOf("job-1", { progress: { current: 128, total: 512, unit: "files" } })],
    });
    render(<JobsPage />);
    await screen.findByRole("progressbar");
    const meter = screen.getByRole("progressbar");
    expect(meter.getAttribute("aria-valuenow")).toBe("25");
    expect(screen.getByText(/128\/512 files · 25%/)).toBeTruthy();
  });

  it("a progress block without a total says what it has — never an invented percent", async () => {
    listJobsMock.mockResolvedValue({
      jobs: [jobOf("job-1", { progress: { current: 7, message: "Scanning plugins" } })],
    });
    render(<JobsPage />);
    await screen.findByText("Scanning plugins");
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("offers Cancel only while it can matter, and reports a refusal through the note", async () => {
    listJobsMock.mockResolvedValue({
      jobs: [
        jobOf("live", { state: "running" }),
        jobOf("done", { state: "succeeded", createdAtMs: 50 }),
      ],
    });
    cancelJobMock.mockRejectedValue(
      new ProtocolRequestError({
        code: "JOB_NOT_CANCELLABLE",
        message: "That job is already finishing on its own",
        remediation: ["Wait for the job to settle"],
      }),
    );
    render(<JobsPage />);
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Cancel" })).toHaveLength(1));
    // The finished job carries no cancel verb at all.
    expect(screen.queryByText("Succeeded")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await screen.findByText("That job is already finishing on its own");
    expect(screen.getByText("Wait for the job to settle")).toBeTruthy();
    expect(cancelJobMock).toHaveBeenCalledWith("live");
  });

  it("a job's own failure renders the daemon's typed error, once", async () => {
    listJobsMock.mockResolvedValue({
      jobs: [
        jobOf("job-1", {
          state: "failed",
          error: {
            code: "CHECKSUM_MISMATCH",
            message: "The downloaded jar did not match its published checksum",
            remediation: ["Retry the download"],
          },
        }),
      ],
    });
    render(<JobsPage />);
    await screen.findByText("The downloaded jar did not match its published checksum");
    // The code hides behind [View details] (§81) but is readable there.
    fireEvent.click(screen.getByText("View details"));
    expect(screen.getByText(/CHECKSUM_MISMATCH/)).toBeTruthy();
    expect(screen.getByText("Retry the download")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  });

  it("the empty state is the honest one — the seed arrived and found nothing", async () => {
    render(<JobsPage />);
    await screen.findByText(/No jobs yet/);
    expect(screen.queryByRole("listitem")).toBeNull();
  });

  it("a seed refusal is a typed alert, not a fake empty", async () => {
    listJobsMock.mockRejectedValue(
      new ProtocolRequestError({
        code: "PROTOCOL_UNAVAILABLE",
        message: "The daemon is not connected",
      }),
    );
    render(<JobsPage />);
    await screen.findByText("The daemon is not connected");
    expect(screen.queryByText(/No jobs yet/)).toBeNull();
  });
});
