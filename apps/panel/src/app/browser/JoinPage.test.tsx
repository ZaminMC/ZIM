// The Join page's verdict copy: each daemon state speaks its own
// recovery path, and a registry server on the port reframes "nothing is
// listening" as "your server is stopped" — the honest overlap, never
// "this is the server you asked for" (the registry stores directories,
// not addresses).

import { describe, expect, it } from "vitest";
import { verdictFor } from "./JoinPage";

const ADDRESS = "192.168.0.1:25565";

describe("join verdict copy", () => {
  it("refused without a registry match offers creation", () => {
    const verdict = verdictFor({ state: "refused" }, ADDRESS);
    expect(verdict.title).toBe("No Server Running on Port 25565");
    expect(verdict.body).toContain(ADDRESS);
    expect(verdict.tone).toBe("down");
  });

  it("refused with a stopped registry server names it", () => {
    const verdict = verdictFor(
      { state: "refused" },
      ADDRESS,
      { serverId: "survival", displayName: "Survival", port: 25565 } as never,
    );
    expect(verdict.title).toContain("stopped");
    expect(verdict.body).toContain("Survival");
  });

  it("alive speaks the ping facts", () => {
    const verdict = verdictFor(
      { state: "alive", version: "1.21", playersOnline: 3, playersMax: 20 },
      ADDRESS,
    );
    expect(verdict.title).toContain("online");
    expect(verdict.tone).toBe("alive");
  });

  it("unreachable, timeout and invalid carry their own truths", () => {
    expect(verdictFor({ state: "unreachable" }, ADDRESS).title).toContain("reach");
    expect(verdictFor({ state: "timeout" }, ADDRESS).title).toContain("did not answer");
    expect(verdictFor({ state: "invalid" }, ADDRESS).title).toContain("not a Minecraft server");
  });
});
