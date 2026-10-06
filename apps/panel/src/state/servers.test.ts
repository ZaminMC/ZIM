import { beforeEach, describe, expect, it } from "vitest";
import { useServers } from "./servers";

function reset() {
  useServers.setState({ servers: {}, crashes: {} });
}

const alpha = { serverId: "alpha", displayName: "Alpha", state: "stopped" as const };
const beta = { serverId: "beta", displayName: "Beta", state: "running" as const };

describe("servers store", () => {
  beforeEach(reset);

  it("replaces everything from a snapshot and prunes stale crash cards", () => {
    const store = useServers.getState();
    store.replaceAll([alpha]);
    store.recordCrash({
      serverId: "alpha",
      phase: "runtime",
      exitCode: 1,
      evidence: "Exception in thread",
      resolved: false,
    });
    store.recordCrash({ serverId: "ghost", phase: "startup", resolved: false });

    // Snapshot without `ghost` prunes its card; a snapshot that says alpha
    // is no longer crashed prunes alpha's card too (fresh truth wins).
    store.replaceAll([alpha, beta]);
    const state = useServers.getState();
    expect(Object.keys(state.servers)).toEqual(["alpha", "beta"]);
    expect(state.crashes["ghost"]).toBeUndefined();
    expect(state.crashes["alpha"]).toBeUndefined();
  });

  it("marks a crash card resolved when the same server transitions again", () => {
    const store = useServers.getState();
    store.replaceAll([alpha]);
    store.applyState("alpha", "crashed");
    store.recordCrash({ serverId: "alpha", phase: "startup", exitCode: 1, resolved: false });
    store.applyState("alpha", "starting");

    expect(useServers.getState().servers["alpha"]?.state).toBe("starting");
    expect(useServers.getState().crashes["alpha"]?.resolved).toBe(true);
  });

  it("applies state changes to the matching server only", () => {
    const store = useServers.getState();
    store.replaceAll([alpha, beta]);
    store.applyState("beta", "stopped");
    const state = useServers.getState();
    expect(state.servers["alpha"]?.state).toBe("stopped");
    expect(state.servers["beta"]?.state).toBe("stopped");
  });

  it("records a crash for an unknown server as a card only", () => {
    const store = useServers.getState();
    store.recordCrash({ serverId: "ghost", phase: "startup", resolved: false });
    const state = useServers.getState();
    expect(state.servers["ghost"]).toBeUndefined();
    expect(state.crashes["ghost"]?.phase).toBe("startup");
  });

  it("forgets a server and its card on remove", () => {
    const store = useServers.getState();
    store.replaceAll([alpha]);
    store.recordCrash({ serverId: "alpha", phase: "runtime", resolved: false });
    store.forget("alpha");
    const state = useServers.getState();
    expect(state.servers["alpha"]).toBeUndefined();
    expect(state.crashes["alpha"]).toBeUndefined();
  });
});
