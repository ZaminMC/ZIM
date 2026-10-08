// The wire boundary's shape guards (P0): a daemon the panel was not
// built for must degrade HERE, at the protocol edge — never inside a
// view. Pinned by the real regression: an older daemon omitted
// `extraJvmArgs` when empty, and the Startup tab died reading `.join`
// off undefined. The action now guarantees the list.

import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  request: vi.fn(),
}));

vi.mock("./wire", () => ({
  client: {
    request: mocks.request,
  },
}));

import { getServerConfig } from "./actions";

const requestMock = mocks.request;

afterEach(() => {
  requestMock.mockReset();
});

describe("getServerConfig — the wire boundary", () => {
  it("an older daemon that omits extraJvmArgs still answers a list", async () => {
    requestMock.mockResolvedValue({
      serverId: "survival",
      displayName: "Survival",
      effective: {
        stopTimeoutSecs: 60,
        startupTimeoutSecs: 120,
        backupKeep: 10,
        // The old wire shape: the field is simply absent when empty.
      },
      provenance: { stopTimeoutSecs: "global" },
    });
    const result = await getServerConfig("survival");
    expect(result.effective.extraJvmArgs).toEqual([]);
  });

  it("a present list passes through untouched", async () => {
    requestMock.mockResolvedValue({
      serverId: "s",
      displayName: "S",
      effective: {
        stopTimeoutSecs: 60,
        startupTimeoutSecs: 120,
        backupKeep: 10,
        extraJvmArgs: ["-XX:+UseG1GC"],
      },
      provenance: {},
    });
    const result = await getServerConfig("s");
    expect(result.effective.extraJvmArgs).toEqual(["-XX:+UseG1GC"]);
  });

  it("a null list degrades to the empty list, never to a crash", async () => {
    requestMock.mockResolvedValue({
      serverId: "s",
      displayName: "S",
      effective: {
        stopTimeoutSecs: 60,
        startupTimeoutSecs: 120,
        backupKeep: 10,
        extraJvmArgs: null,
      },
      provenance: {},
    });
    const result = await getServerConfig("s");
    expect(result.effective.extraJvmArgs).toEqual([]);
  });
});
