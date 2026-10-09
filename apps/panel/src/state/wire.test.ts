// The wire's tests: the one-client glue's event routing — which events
// land in which stores, the crash card with its focus-gated notification,
// the registered event's fetch-then-upsert, and the snapshot that seeds
// the fleet. The stores are the real zustand instances; the client and
// the notification lane are the mocks.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../protocol/client", () => ({
  ProtocolClient: class {
    connect = vi.fn().mockResolvedValue(undefined);
    reconnect = vi.fn().mockResolvedValue(undefined);
    setAuth = vi.fn();
    onStatus = vi.fn().mockReturnValue(() => {});
    subscribe = vi.fn().mockResolvedValue({ dispose: vi.fn(), result: null });
  },
}));

vi.mock("../protocol/transport", () => ({
  TauriTransport: class {},
  WsTransport: class {},
}));

vi.mock("../integration/notifications", () => ({
  isUnfocused: vi.fn().mockReturnValue(true),
  shouldNotify: vi.fn().mockReturnValue(true),
  deliver: vi.fn().mockResolvedValue(undefined),
  crashNotification: vi.fn((x: unknown) => x),
  jobNotification: vi.fn((x: unknown) => x),
}));

vi.mock("./actions", () => ({
  getServer: vi.fn(),
}));

import { startWire, reconnectWire, client } from "./wire";
import { useServers } from "./servers";
import { useJobs } from "./jobs";
import { getServer } from "./actions";
import { deliver, shouldNotify } from "../integration/notifications";

const subscribeMock = client.subscribe as unknown as ReturnType<typeof vi.fn>;
const connectMock = client.connect as unknown as ReturnType<typeof vi.fn>;
const reconnectMock = client.reconnect as unknown as ReturnType<typeof vi.fn>;
const getServerMock = getServer as unknown as ReturnType<typeof vi.fn>;
const deliverMock = deliver as unknown as ReturnType<typeof vi.fn>;
const notifyGateMock = shouldNotify as unknown as ReturnType<typeof vi.fn>;

type Handler = {
  onPayload: (n: { payload: unknown }) => void;
  onRegistered?: (result: { snapshot?: { servers: unknown[] } }) => void;
};

function eventHandler(): Handler {
  // wire.ts calls client.subscribe("events", undefined, handlers) — the
  // handlers are the third argument.
  const call = subscribeMock.mock.calls.find((c) => c[0] === "events");
  return call?.[2] as Handler;
}

function pushEvent(event: Record<string, unknown>): void {
  eventHandler().onPayload({ payload: { kind: "event", event } });
}

beforeEach(() => {
  // The wire owns ONE events stream for the app's lifetime (startWire is
  // idempotent by a module flag), so the subscribe record persists across
  // tests — every test routes through the same captured handler. Only the
  // per-verb mocks and the stores reset.
  connectMock.mockClear();
  reconnectMock.mockClear();
  getServerMock.mockReset();
  deliverMock.mockClear();
  // The focus gate returns to "away" — a test that mutes notifications
  // must not leak its silence into the next one.
  notifyGateMock.mockReset().mockReturnValue(true);
  useServers.setState({ servers: {}, crashes: {} });
  useJobs.setState({ jobs: {} });
});

afterEach(() => {
  useServers.setState({ servers: {}, crashes: {} });
  useJobs.setState({ jobs: {} });
});

describe("the wire", () => {
  it("startWire binds, connects once, and opens the events stream — idempotently", () => {
    startWire();
    startWire();
    startWire();
    expect(connectMock).toHaveBeenCalledTimes(1);
    expect(subscribeMock).toHaveBeenCalledTimes(1);
    expect(subscribeMock.mock.calls[0]?.[0]).toBe("events");
  });

  it("the registration snapshot seeds the fleet in one replaceAll", () => {
    startWire();
    eventHandler().onRegistered?.({
      snapshot: { servers: [{ serverId: "s1", displayName: "One", state: "stopped" }] },
    });
    expect(useServers.getState().servers["s1"]?.displayName).toBe("One");
  });

  it("a state change applies to the matching server; an unknown id is still recorded", () => {
    startWire();
    useServers.setState({
      servers: { s1: { serverId: "s1", displayName: "One", state: "starting" } },
    });
    pushEvent({ type: "serverStateChanged", serverId: "s1", to: "running" });
    expect(useServers.getState().servers["s1"]?.state).toBe("running");
  });

  it("a removed server is forgotten — its metrics with it", () => {
    startWire();
    useServers.setState({
      servers: { s1: { serverId: "s1", displayName: "One", state: "stopped" } },
    });
    pushEvent({ type: "serverStateChanged", serverId: "s1", to: "stopped", reason: "removed" });
    expect(useServers.getState().servers["s1"]).toBeUndefined();
  });

  it("a registered server fetches its details and upserts them", async () => {
    getServerMock.mockResolvedValue({ serverId: "s2", displayName: "Two", state: "stopped", port: 25565 });
    startWire();
    pushEvent({ type: "serverStateChanged", serverId: "s2", to: "adopting", reason: "registered" });
    await vi.waitFor(() => expect(getServerMock).toHaveBeenCalledWith("s2"));
    await vi.waitFor(() => {
      expect(useServers.getState().servers["s2"]?.displayName).toBe("Two");
    });
  });

  it("a crash records the crash card and notifies the away operator", () => {
    startWire();
    pushEvent({
      type: "serverStateChanged",
      serverId: "s1",
      to: "crashed",
      crash: { phase: "runtime", exitCode: 1, evidence: "stack overflow" },
    });
    const crash = useServers.getState().crashes["s1"];
    expect(crash?.phase).toBe("runtime");
    expect(crash?.exitCode).toBe(1);
    expect(crash?.resolved).toBe(false);
    expect(deliverMock).toHaveBeenCalledTimes(1);
  });

  it("an operator looking at the window gets the crash card without the notification", async () => {
    notifyGateMock.mockReturnValue(false);
    startWire();
    pushEvent({
      type: "serverStateChanged",
      serverId: "s1",
      to: "crashed",
      crash: { phase: "runtime", exitCode: 1 },
    });
    expect(useServers.getState().crashes["s1"]).toBeTruthy();
    expect(deliverMock).not.toHaveBeenCalled();
  });

  it("job events land in the jobs store; completion notifies once", () => {
    startWire();
    pushEvent({
      type: "jobStarted",
      job: { jobId: "j1", kind: "backup.create", serverId: "s1", state: "running", createdAtMs: 1 },
    });
    expect(useJobs.getState().jobs["j1"]?.kind).toBe("backup.create");
    pushEvent({ type: "jobProgress", jobId: "j1", progress: { current: 3, total: 9 } });
    expect(useJobs.getState().jobs["j1"]?.progress?.current).toBe(3);
    expect(deliverMock).not.toHaveBeenCalled();
    pushEvent({ type: "jobCompleted", jobId: "j1", outcome: "succeeded" });
    expect(useJobs.getState().jobs["j1"]?.state).toBe("succeeded");
    expect(deliverMock).toHaveBeenCalledTimes(1);
  });

  it("events outside the routed families are ignored", () => {
    startWire();
    pushEvent({ type: "metricsTick", serverId: "s1" });
    pushEvent({ type: "logsAppended" });
    expect(deliverMock).not.toHaveBeenCalled();
    expect(useServers.getState().servers).toEqual({});
  });

  it("reconnectWire re-opens the wire for the active profile", () => {
    startWire();
    reconnectWire();
    expect(reconnectMock).toHaveBeenCalledTimes(1);
  });
});
