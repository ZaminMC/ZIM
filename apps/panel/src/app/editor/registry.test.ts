// The specialized editor registry (§34): a file is claimed by the
// strongest bidder, and the generic Compose view answers for everything
// the registry cannot speak for. Also: the test seam must refill the
// registry — a test cannot leave the panel rerouted.

import { describe, expect, it } from "vitest";
import { specializedEditorFor, withEditors, type SpecializedEditor } from "./registry";

describe("specializedEditorFor", () => {
  it("routes a scoreboard.properties file by name", () => {
    const editor = specializedEditorFor("plugins/Hud/scoreboard.properties", "title=T\n");
    expect(editor?.id).toBe("scoreboard");
  });

  it("routes by content shape — title pair plus row keys", () => {
    const editor = specializedEditorFor("plugins/Hud/hud.properties", "title=T\nline.1=A\nline.2=B\n");
    expect(editor?.id).toBe("scoreboard");
  });

  it("claims nothing for server.properties", () => {
    const text = "server-port=25565\nmotd=A server\nmax-players=20\n";
    expect(specializedEditorFor("server.properties", text)).toBeNull();
  });

  it("claims nothing for non-properties files", () => {
    expect(specializedEditorFor("scoreboard.yml", "title: T\n")).toBeNull();
  });

  it("claims nothing while no file is open", () => {
    expect(specializedEditorFor(null, "")).toBeNull();
  });

  it("the strongest bidder wins", () => {
    const strong: SpecializedEditor = { id: "strong", label: "S", strength: () => 1 };
    const weak: SpecializedEditor = { id: "weak", label: "W", strength: () => 0.5 };
    withEditors([weak, strong], () => {
      expect(specializedEditorFor("anything.txt", "")?.id).toBe("strong");
    });
  });
});

describe("withEditors", () => {
  it("refills the registry afterwards — no test leaves the panel rerouted", () => {
    const fake: SpecializedEditor[] = [{ id: "fake", label: "F", strength: () => 1 }];
    withEditors(fake, () => {
      expect(specializedEditorFor("whatever", "")?.id).toBe("fake");
    });
    // The real registry is back: a plain text file claims nothing.
    expect(specializedEditorFor("notes.txt", "hello")).toBeNull();
  });
});
