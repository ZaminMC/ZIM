// Schedules: named, timed tasks the daemon itself fires — restarts,
// backups, console lines (ADR-0014). The panel only authors the rules and
// reads the record: lastFiredMs is the clock's memory, nextRunMs its
// hint. Times are the daemon's own clock (matters for remote profiles —
// the box may live in another zone than this window).

import { useCallback, useEffect, useState } from "react";
import {
  createSchedule,
  deleteSchedule,
  listSchedules,
  updateSchedule,
} from "../state/actions";
import { describeError } from "../state/errors";
import type { DescribedError } from "../state/errors";
import { ErrorNote } from "../ui/ErrorNote";
import type { ScheduleAction, ScheduleSpec, ScheduleView } from "../protocol/types";
import { Button } from "../ui/Button";
import styles from "./SchedulesView.module.css";

const WEEKDAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] as const;

// The clock's memory (lastFiredMs) and its hint (nextRunMs) change while
// this tab is open — a schedule due at 04:30 fires at 04:30, not on the
// next navigation. A poll on the Players tab's cadence keeps the rows
// honest; the daemon's tick is 15 s, so 10 s never lags a firing long.
const REFRESH_MS = 10_000;

function formatEvery(secs: number): string {
  if (secs % 3600 === 0) return `${secs / 3600} h`;
  if (secs % 60 === 0) return `${secs / 60} min`;
  return `${secs} s`;
}

function describeSpec(spec: ScheduleSpec): string {
  if (spec.kind === "interval") return `every ${formatEvery(spec.everySecs)}`;
  if (spec.kind === "daily") return `daily at ${spec.at}`;
  const days = spec.weekdays
    .map((day) => day.charAt(0).toUpperCase() + day.slice(1))
    .join(", ");
  return `${days} at ${spec.at}`;
}

function describeAction(action: ScheduleAction): string {
  if (action.kind === "restart") return "Restart the server";
  if (action.kind === "backup") return "Take a backup";
  return `Console: ${action.line}`;
}

function formatWhen(ms: number | undefined): string {
  return ms === undefined ? "never" : new Date(ms).toLocaleString();
}

type SpecDraft = { kind: "interval"; everySecs: number } | { kind: "daily"; at: string } | {
  kind: "weekly";
  weekdays: string[];
  at: string;
};

export function SchedulesView({ serverId }: { serverId: string }) {
  const [schedules, setSchedules] = useState<ScheduleView[] | null>(null);
  const [error, setError] = useState<DescribedError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [adding, setAdding] = useState(false);
  const [confirming, setConfirming] = useState<string | null>(null);

  // The add form's fields (one controlled bundle).
  const [name, setName] = useState("");
  const [specKind, setSpecKind] = useState<"interval" | "daily" | "weekly">("daily");
  const [everySecs, setEverySecs] = useState(21600);
  const [at, setAt] = useState("04:30");
  const [weekdays, setWeekdays] = useState<string[]>(["mon"]);
  const [actionKind, setActionKind] = useState<"restart" | "backup" | "command">("restart");
  const [commandLine, setCommandLine] = useState("");

  const refresh = useCallback(() => {
    void listSchedules(serverId)
      .then((result) => {
        setSchedules(result.schedules);
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause)));
  }, [serverId]);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const draft = (): ScheduleDraft | string => {
    if (name.trim() === "") return "The schedule needs a name.";
    let spec: SpecDraft;
    if (specKind === "interval") {
      if (!Number.isFinite(everySecs) || everySecs < 300) {
        return "Intervals are measured in seconds; the panel starts at 5 minutes.";
      }
      spec = { kind: "interval", everySecs: Math.floor(everySecs) };
    } else if (specKind === "daily") {
      if (!/^\d{2}:\d{2}$/.test(at)) return `"${at}" is not a 24-hour HH:MM time.`;
      spec = { kind: "daily", at };
    } else {
      if (!/^\d{2}:\d{2}$/.test(at)) return `"${at}" is not a 24-hour HH:MM time.`;
      if (weekdays.length === 0) return "Pick at least one weekday.";
      spec = { kind: "weekly", weekdays: [...weekdays], at };
    }
    let action: ScheduleAction;
    if (actionKind === "command") {
      if (commandLine.trim() === "") return "A command schedule needs a console line.";
      action = { kind: "command", line: commandLine.trim() };
    } else {
      action = { kind: actionKind };
    }
    return { name: name.trim(), spec, action, enabled: true };
  };

  const runCreate = () => {
    setError(null);
    setNotice(null);
    const built = draft();
    if (typeof built === "string") {
      setError({ title: built, remediation: [] });
      return;
    }
    setBusy(true);
    void createSchedule(serverId, built)
      .then(() => {
        setNotice("Schedule added — it runs on its next tick.");
        setAdding(false);
        setName("");
      })
      .catch((cause: unknown) => setError(describeError(cause)))
      .finally(() => setBusy(false));
  };

  const runToggle = (schedule: ScheduleView) => {
    setError(null);
    setNotice(null);
    setBusy(true);
    void updateSchedule(serverId, schedule.id, { enabled: !schedule.enabled })
      .then((result) =>
        setNotice(
          result.schedule.enabled
            ? "Schedule resumed — the clock fires it when due."
            : "Schedule paused — nothing fires until you resume it.",
        ),
      )
      .catch((cause: unknown) => setError(describeError(cause)))
      .finally(() => setBusy(false));
  };

  const runDelete = (scheduleId: string) => {
    setError(null);
    setNotice(null);
    setConfirming(null);
    setBusy(true);
    void deleteSchedule(serverId, scheduleId)
      .then(() => setNotice("Schedule removed."))
      .catch((cause: unknown) => setError(describeError(cause)))
      .finally(() => setBusy(false));
  };

  return (
    <section className={styles.schedules} aria-label="Schedules">
      <div className={styles.head}>
        <span className={styles.count}>
          {schedules ? `${schedules.length} schedules` : "loading…"}
        </span>
        <span className={styles.spacer} />
        <Button
          onClick={() => {
            setAdding((value) => !value);
            setError(null);
          }}
        >
          {adding ? "Close form" : "Add schedule"}
        </Button>
      </div>

      <p className={styles.note}>
        ZIM fires these itself — restarts, backups, console lines. Times are ZIM's
        own clock, and a schedule never switches a stopped server on.
      </p>

      {error ? (
        <div className={styles.alert} role="alert">
        <ErrorNote error={error} />
      </div>
      ) : null}
      {notice && !error ? (
        <div className={styles.notice} role="status">
          {notice}
        </div>
      ) : null}

      {adding ? (
        <form
          className={styles.form}
          onSubmit={(event) => {
            event.preventDefault();
            runCreate();
          }}
        >
          <div className={styles.formRow}>
            <label className={styles.field}>
              <span>Name</span>
              <input
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="nightly restart"
              />
            </label>
            <label className={styles.field}>
              <span>When</span>
              <select
                value={specKind}
                onChange={(event) =>
                  setSpecKind(event.target.value as "interval" | "daily" | "weekly")
                }
              >
                <option value="daily">Every day at…</option>
                <option value="weekly">Weeks at…</option>
                <option value="interval">Fixed interval</option>
              </select>
            </label>
            {specKind === "interval" ? (
              <label className={styles.field}>
                <span>Every (seconds)</span>
                <input
                  type="number"
                  min={300}
                  value={everySecs}
                  onChange={(event) => setEverySecs(Number(event.target.value))}
                />
              </label>
            ) : (
              <label className={styles.field}>
                <span>At (HH:MM)</span>
                <input value={at} onChange={(event) => setAt(event.target.value)} placeholder="04:30" />
              </label>
            )}
          </div>
          {specKind === "weekly" ? (
            <div className={styles.formRow} role="group" aria-label="Weekdays">
              {WEEKDAYS.map((day) => (
                <label key={day} className={styles.dayChip}>
                  <input
                    type="checkbox"
                    checked={weekdays.includes(day)}
                    onChange={(event) =>
                      setWeekdays((current) =>
                        event.target.checked
                          ? [...current, day]
                          : current.filter((value) => value !== day),
                      )
                    }
                  />
                  {day}
                </label>
              ))}
            </div>
          ) : null}
          <div className={styles.formRow}>
            <label className={styles.field}>
              <span>Then</span>
              <select
                value={actionKind}
                onChange={(event) =>
                  setActionKind(event.target.value as "restart" | "backup" | "command")
                }
              >
                <option value="restart">Restart the server</option>
                <option value="backup">Take a backup</option>
                <option value="command">Send a console line</option>
              </select>
            </label>
            {actionKind === "command" ? (
              <label className={styles.fieldGrow}>
                <span>Console line</span>
                <input
                  value={commandLine}
                  onChange={(event) => setCommandLine(event.target.value)}
                  placeholder="say Restarting in 10 minutes"
                />
              </label>
            ) : null}
          </div>
          <div className={styles.formActions}>
            <Button type="submit" disabled={busy}>
              Add schedule
            </Button>
            <Button onClick={() => setAdding(false)}>Cancel</Button>
          </div>
        </form>
      ) : null}

      {schedules && schedules.length === 0 ? (
        <p className={styles.empty}>
          No schedules yet. A nightly restart or an hourly backup takes one row — the daemon does
          the remembering.
        </p>
      ) : null}

      {schedules && schedules.length > 0 ? (
        <ul className={styles.list}>
          {schedules.map((schedule) => (
            <li key={schedule.id} className={styles.item}>
              <div className={styles.itemMain}>
                <span className={styles.name}>
                  {schedule.name}
                  {!schedule.enabled ? <span className={styles.paused}>paused</span> : null}
                </span>
                <span className={styles.meta}>
                  {describeSpec(schedule.spec)} — {describeAction(schedule.action)}
                </span>
                <span className={styles.meta}>
                  last fired {formatWhen(schedule.lastFiredMs)}
                  {schedule.enabled && schedule.nextRunMs !== undefined
                    ? ` · next ${formatWhen(schedule.nextRunMs)}`
                    : ""}
                </span>
              </div>
              {confirming === schedule.id ? (
                <div className={styles.confirm}>
                  <span>Remove “{schedule.name}”?</span>
                  <Button variant="danger" disabled={busy} onClick={() => runDelete(schedule.id)}>
                    Yes, remove
                  </Button>
                  <Button onClick={() => setConfirming(null)}>Keep it</Button>
                </div>
              ) : (
                <div className={styles.itemActions}>
                  <Button
                    disabled={busy}
                    onClick={() => runToggle(schedule)}
                    title={
                      schedule.enabled
                        ? "Pause: the clock skips this schedule entirely"
                        : "Resume: the clock fires it when due"
                    }
                  >
                    {schedule.enabled ? "Pause" : "Resume"}
                  </Button>
                  <Button variant="danger" disabled={busy} onClick={() => setConfirming(schedule.id)}>
                    Remove
                  </Button>
                </div>
              )}
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

interface ScheduleDraft {
  name: string;
  spec: ScheduleSpec;
  action: ScheduleAction;
  enabled: boolean;
}
