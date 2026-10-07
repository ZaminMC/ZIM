// Schedules view regressions: rows render the stored record (spec, action,
// last fired / next run), the toggle dispatches the flip, delete confirms
// first, the add form validates locally and dispatches the draft, and
// daemon-typed errors surface through describeError.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SchedulesView } from "./SchedulesView";
import type { ScheduleView } from "../protocol/types";

vi.mock("../state/actions", () => ({
  listSchedules: vi.fn(),
  createSchedule: vi.fn(),
  updateSchedule: vi.fn(),
  deleteSchedule: vi.fn(),
}));

import {
  createSchedule,
  deleteSchedule,
  listSchedules,
  updateSchedule,
} from "../state/actions";

const listSchedulesMock = listSchedules as ReturnType<typeof vi.fn>;
const createScheduleMock = createSchedule as ReturnType<typeof vi.fn>;
const updateScheduleMock = updateSchedule as ReturnType<typeof vi.fn>;
const deleteScheduleMock = deleteSchedule as ReturnType<typeof vi.fn>;

function scheduleOf(id: string, extra: Partial<ScheduleView> = {}): ScheduleView {
  return {
    id,
    name: `schedule ${id}`,
    spec: { kind: "daily", at: "04:30" },
    action: { kind: "restart" },
    enabled: true,
    createdMs: 1_730_803_200_000,
    ...extra,
  };
}

beforeEach(() => {
  listSchedulesMock.mockReset().mockResolvedValue({ serverId: "demo", schedules: [] });
  createScheduleMock.mockReset().mockResolvedValue({
    serverId: "demo",
    schedule: scheduleOf("new"),
  });
  updateScheduleMock.mockReset().mockImplementation((_serverId: string, id: string, patch: { enabled?: boolean }) =>
    Promise.resolve({
      serverId: "demo",
      schedule: scheduleOf(id, { enabled: patch.enabled ?? true }),
    }),
  );
  deleteScheduleMock.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

describe("SchedulesView", () => {
  it("renders the stored rows with their spec, action, and clock memory", async () => {
    listSchedulesMock.mockResolvedValue({
      serverId: "demo",
      schedules: [
        scheduleOf("a", {
          nextRunMs: 1_730_860_200_000,
          lastFiredMs: 1_730_851_560_000,
        }),
        scheduleOf("b", {
          enabled: false,
          spec: { kind: "interval", everySecs: 21600 },
          action: { kind: "backup" },
        }),
        scheduleOf("c", {
          action: { kind: "command", line: "say Restarting soon" },
        }),
      ],
    });

    render(<SchedulesView serverId="demo" />);

    await waitFor(() => expect(screen.getByText("schedule a")).toBeTruthy());
    expect(screen.getByText("3 schedules")).toBeTruthy();
    expect(screen.getByText(/daily at 04:30 — Restart the server/)).toBeTruthy();
    expect(screen.getAllByText(/last fired/).length).toBe(3);
    expect(screen.getByText(/next /)).toBeTruthy();
    expect(screen.getByText("schedule b")).toBeTruthy();
    expect(screen.getByText(/every 6 h — Take a backup/)).toBeTruthy();
    expect(screen.getByText("paused")).toBeTruthy();
    expect(screen.getByText(/Console: say Restarting soon/)).toBeTruthy();
  });

  it("pauses and resumes through the update verb", async () => {
    listSchedulesMock.mockResolvedValue({
      serverId: "demo",
      schedules: [scheduleOf("a")],
    });
    render(<SchedulesView serverId="demo" />);
    await waitFor(() => expect(screen.getByText("schedule a")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    await waitFor(() =>
      expect(updateScheduleMock).toHaveBeenCalledWith("demo", "a", { enabled: false }),
    );
  });

  it("confirms before removing a schedule", async () => {
    listSchedulesMock.mockResolvedValue({
      serverId: "demo",
      schedules: [scheduleOf("a")],
    });
    render(<SchedulesView serverId="demo" />);
    await waitFor(() => expect(screen.getByText("schedule a")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(deleteScheduleMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Yes, remove" }));
    await waitFor(() => expect(deleteScheduleMock).toHaveBeenCalledWith("demo", "a"));
  });

  it("the add form validates locally and dispatches the draft", async () => {
    listSchedulesMock.mockResolvedValue({ serverId: "demo", schedules: [] });
    render(<SchedulesView serverId="demo" />);

    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));
    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));

    // Empty name: a local refusal, no round trip.
    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));
    expect(screen.getByRole("alert").textContent).toContain("needs a name");
    expect(createScheduleMock).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "nightly restart" },
    });
    fireEvent.change(screen.getByLabelText("At (HH:MM)"), {
      target: { value: "04:30" },
    });
    fireEvent.change(screen.getByLabelText("Then"), { target: { value: "restart" } });
    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));

    await waitFor(() =>
      expect(createScheduleMock).toHaveBeenCalledWith("demo", {
        name: "nightly restart",
        spec: { kind: "daily", at: "04:30" },
        action: { kind: "restart" },
        enabled: true,
      }),
    );
  });

  it("weekly picks land as the weekdays list", async () => {
    listSchedulesMock.mockResolvedValue({ serverId: "demo", schedules: [] });
    render(<SchedulesView serverId="demo" />);

    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "weekend" } });
    fireEvent.change(screen.getByLabelText("When"), { target: { value: "weekly" } });
    fireEvent.change(screen.getByLabelText("At (HH:MM)"), { target: { value: "09:00" } });
    fireEvent.click(screen.getByLabelText("mon")); // the form pre-checks one day
    fireEvent.click(screen.getByLabelText("sat"));
    fireEvent.click(screen.getByLabelText("sun"));
    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));

    await waitFor(() =>
      expect(createScheduleMock).toHaveBeenCalledWith("demo", {
        name: "weekend",
        spec: { kind: "weekly", weekdays: ["sat", "sun"], at: "09:00" },
        action: { kind: "restart" },
        enabled: true,
      }),
    );
  });

  it("surfaces the daemon's typed refusal", async () => {
    listSchedulesMock.mockResolvedValue({ serverId: "demo", schedules: [] });
    createScheduleMock.mockRejectedValue(
      Object.assign(new Error("The schedule is invalid: …"), {
        error: { code: "SCHEDULE_INVALID", message: "The schedule is invalid: bad time." },
      }),
    );
    render(<SchedulesView serverId="demo" />);

    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "broken" } });
    fireEvent.click(screen.getByRole("button", { name: "Add schedule" }));

    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  });
});
