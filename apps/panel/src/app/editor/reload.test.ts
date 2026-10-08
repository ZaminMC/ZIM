// §35's reload knowledge: the plan comes from the path, and the honest
// answer for the JVM configs is the restart — /reload is deliberately
// absent because it is the one verb that pretends. Files with no story
// get no story.

import { describe, expect, it } from "vitest";
import { reloadPlanFor } from "./reload";

describe("reload plan", () => {
  it("server.properties loads at boot — the plan says restart, honestly", () => {
    const plan = reloadPlanFor("server.properties");
    expect(plan.restartable).toBe(true);
    expect(plan.explain).toContain("boot");
  });

  it("the JVM config family gets the same honest answer", () => {
    for (const path of [
      "bukkit.yml",
      "spigot.yml",
      "paper-global.yml",
      "config/paper-global.yml",
      "paper-world-defaults.yml",
      "purpur.yml",
    ]) {
      expect(reloadPlanFor(path).restartable, path).toBe(true);
    }
  });

  it("plugin configs get the restart, with the no-fake-reload sentence", () => {
    const plan = reloadPlanFor("plugins/EssentialsX/config.yml");
    expect(plan.restartable).toBe(true);
    expect(plan.explain).toContain("does not fake a plugin reload");
  });

  it("a file with no story gets no story", () => {
    const plan = reloadPlanFor("logs/latest.log");
    expect(plan.restartable).toBe(false);
    expect(plan.explain).toBeUndefined();
  });
});
