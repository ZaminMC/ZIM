// Palette command building: context-aware verbs per the lifecycle ladder.

import { describe, expect, it } from "vitest";
import { buildCommands } from "./Palette";

describe("buildCommands", () => {
  it("always offers registration", () => {
    const commands = buildCommands(null, null, { newServer: () => {}, lifecycle: () => {} });
    expect(commands.find((command) => command.id === "new-server")).toBeTruthy();
  });

  it("offers only state-legal lifecycle verbs for the active server", () => {
    const commands = buildCommands("alpha", "not-running", {
      newServer: () => {},
      lifecycle: () => {},
    });
    const byId = (id: string) => commands.find((command) => command.id === id);
    expect(byId("verb-start")?.disabled).toBe(false);
    expect(byId("verb-stop")?.disabled).toBe(true);
    expect(byId("verb-restart")?.disabled).toBe(true);
    expect(byId("verb-kill")?.disabled).toBe(true);
  });

  it("enables stop/restart/kill while running and disables start", () => {
    const commands = buildCommands("alpha", "running", {
      newServer: () => {},
      lifecycle: () => {},
    });
    const byId = (id: string) => commands.find((command) => command.id === id);
    expect(byId("verb-start")?.disabled).toBe(true); // already running
    expect(byId("verb-stop")?.disabled).toBe(false);
    expect(byId("verb-restart")?.disabled).toBe(false);
    expect(byId("verb-kill")?.disabled).toBe(false);
  });

  it("carries the evidence pages as keyboard commands (§58, ADR-0026)", () => {
    const ran: string[] = [];
    const commands = buildCommands(
      null,
      null,
      { newServer: () => {}, lifecycle: () => {} },
      undefined,
      {
        jobs: () => ran.push("jobs"),
        audit: () => ran.push("audit"),
        about: () => ran.push("about"),
        feedback: () => ran.push("feedback"),
      },
    );
    const jobs = commands.find((command) => command.id === "page-jobs");
    const audit = commands.find((command) => command.id === "page-audit");
    const about = commands.find((command) => command.id === "page-about");
    const feedback = commands.find((command) => command.id === "page-feedback");
    jobs?.run();
    audit?.run();
    about?.run();
    feedback?.run();
    expect(ran).toEqual(["jobs", "audit", "about", "feedback"]);
    expect(jobs?.hint).toBe("zaminpanel://jobs/");
    expect(feedback?.hint).toBe("zaminpanel://feedback/");
  });

  it("hides the autostart command where the host cannot deliver it", () => {
    const commands = buildCommands(
      null,
      null,
      { newServer: () => {}, lifecycle: () => {} },
      { autostart: { available: false }, toggleAutostart: () => {} },
    );
    expect(commands.find((command) => command.id === "autostart")).toBeUndefined();
  });

  it("offers the autostart toggle with the honest label for the current state", () => {
    const off = buildCommands(
      null,
      null,
      { newServer: () => {}, lifecycle: () => {} },
      { autostart: { available: true, enabled: false }, toggleAutostart: () => {} },
    );
    const offCommand = off.find((command) => command.id === "autostart");
    expect(offCommand?.label).toBe("Start with the system");

    const on = buildCommands(
      null,
      null,
      { newServer: () => {}, lifecycle: () => {} },
      { autostart: { available: true, enabled: true }, toggleAutostart: () => {} },
    );
    const onCommand = on.find((command) => command.id === "autostart");
    expect(onCommand?.label).toBe("Stop starting with the system");
  });

  it("runs the toggle through the command", () => {
    let toggled = 0;
    const commands = buildCommands(
      null,
      null,
      { newServer: () => {}, lifecycle: () => {} },
      { autostart: { available: true, enabled: false }, toggleAutostart: () => (toggled += 1) },
    );
    commands.find((command) => command.id === "autostart")?.run();
    expect(toggled).toBe(1);
  });
});
