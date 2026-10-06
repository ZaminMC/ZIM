// Backups view regressions: the list renders newest-first with the taken
// mode, create dispatches the job, restore requires the stop-first
// confirmation and is disabled while running, and a running job renders
// live progress from the jobs store.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BackupsView } from "./BackupsView";
import { useJobs } from "../state/jobs";
import type { BackupInfo, Job } from "../protocol/types";

vi.mock("../state/actions", () => ({
  listBackups: vi.fn(),
  createBackup: vi.fn(),
  restoreBackup: vi.fn(),
}));

import { createBackup, listBackups, restoreBackup } from "../state/actions";

const listBackupsMock = listBackups as ReturnType<typeof vi.fn>;
const createBackupMock = createBackup as ReturnType<typeof vi.fn>;
const restoreBackupMock = restoreBackup as ReturnType<typeof vi.fn>;

function backupOf(id: string, extra: Partial<BackupInfo> = {}): BackupInfo {
  return {
    backupId: id,
    createdAtMs: 1_730_803_200_000,
    sizeBytes: 4096,
    totalBytes: 65_536,
    fileCount: 12,
    taken: "cold",
    ...extra,
  };
}

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
  listBackupsMock.mockReset().mockResolvedValue({ backups: [] });
  createBackupMock.mockReset().mockResolvedValue({
    kind: "backup.create",
    job: jobOf("job-1"),
  });
  restoreBackupMock.mockReset().mockResolvedValue({
    kind: "backup.restore",
    job: jobOf("job-2", { kind: "backup.restore" }),
  });
});

afterEach(() => {
  cleanup();
  useJobs.setState({ jobs: {} });
});

describe("BackupsView", () => {
  it("renders the empty state", async () => {
    listBackupsMock.mockResolvedValue({ backups: [] });
    render(<BackupsView serverId="demo" running={false} />);
    await waitFor(() => expect(screen.getByText(/No backups yet/)).toBeTruthy());
    expect(listBackupsMock).toHaveBeenCalledWith("demo");
  });

  it("lists backups with mode, size, and label", async () => {
    listBackupsMock.mockResolvedValue({
      backups: [
        backupOf("b-2", { label: "before-map-reset", taken: "live" }),
        backupOf("b-1"),
      ],
    });
    render(<BackupsView serverId="demo" running={false} />);
    await waitFor(() => expect(screen.getByText("2 backups")).toBeTruthy());
    expect(screen.getByText("before-map-reset")).toBeTruthy();
    expect(screen.getByText(/4.0 KiB · 12 files · live/)).toBeTruthy();
    // The restore affordance exists per row.
    expect(screen.getAllByText("Restore").length).toBe(2);
  });

  it("create dispatches a backup job and shows the started notice", async () => {
    render(<BackupsView serverId="demo" running={false} />);
    await waitFor(() => expect(screen.getByText("Create backup")).toBeTruthy());
    fireEvent.click(screen.getByText("Create backup"));
    await waitFor(() => expect(createBackupMock).toHaveBeenCalledWith("demo"));
    await waitFor(() =>
      expect(screen.getByText(/Backup started/)).toBeTruthy(),
    );
  });

  it("restore is disabled while the server is running, with the reason", async () => {
    listBackupsMock.mockResolvedValue({ backups: [backupOf("b-1")] });
    render(<BackupsView serverId="demo" running={true} />);
    const button = await screen.findByText<HTMLButtonElement>("Restore");
    expect(button.disabled).toBe(true);
    expect(button.title).toContain("Stop the server");
    // Create stays available: the daemon wraps it in the save window.
    const create = screen.getByText<HTMLButtonElement>("Create backup");
    expect(create.disabled).toBe(false);
  });

  it("restore asks for confirmation before dispatching", async () => {
    listBackupsMock.mockResolvedValue({ backups: [backupOf("b-1")] });
    render(<BackupsView serverId="demo" running={false} />);
    fireEvent.click(await screen.findByText("Restore"));
    expect(restoreBackupMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Yes, restore"));
    await waitFor(() => expect(restoreBackupMock).toHaveBeenCalledWith("demo", "b-1"));
  });

  it("shows live progress from the jobs store", () => {
    useJobs.getState().started(jobOf("job-9", { progress: { current: 2048, total: 4096, unit: "bytes" } }));
    listBackupsMock.mockResolvedValue({ backups: [] });
    render(<BackupsView serverId="demo" running={false} />);
    expect(screen.getByText("Backup")).toBeTruthy();
    expect(screen.getByText("50%")).toBeTruthy();
    const create = screen.getByText<HTMLButtonElement>("Backing up…");
    expect(create.disabled).toBe(true);
  });

  it("surfaces a failed job as a notice and re-lists", async () => {
    listBackupsMock.mockResolvedValue({ backups: [] });
    render(<BackupsView serverId="demo" running={false} />);
    useJobs.getState().started(jobOf("job-3"));
    useJobs.getState().completed("job-3", "failed", { code: "DISK_FULL", message: "the disk is full" });
    await waitFor(() =>
      expect(screen.getByText(/The last backup failed/)).toBeTruthy(),
    );
    // The completion re-listed the archives.
    await waitFor(() => expect(listBackupsMock.mock.calls.length).toBeGreaterThanOrEqual(2));
  });
});
