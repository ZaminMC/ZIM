// Protocol client unit tests against a scripted mock transport. The wire
// shapes here double as fixtures for zamin-protocol's serde output.

import { describe, expect, it, vi } from "vitest";
import { beforeEach, afterEach } from "vitest";
import {
  ConnectionLostError,
  ProtocolClient,
  ProtocolRequestError,
  RequestTimeoutError,
} from "./client";
import type { Transport } from "./transport";

class MockTransport implements Transport {
  sent: string[] = [];
  started = 0;
  stopped = 0;
  startError: Error | null = null;
  private sink: ((batch: readonly string[]) => void) | null = null;
  private down: (() => void) | null = null;

  start(sink: (batch: readonly string[]) => void, down: () => void): Promise<void> {
    this.started += 1;
    if (this.startError) {
      return Promise.reject(this.startError);
    }
    this.sink = sink;
    this.down = down;
    return Promise.resolve();
  }

  send(frame: string): void {
    this.sent.push(frame);
  }

  stop(): Promise<void> {
    this.stopped += 1;
    this.sink = null;
    this.down = null;
    return Promise.resolve();
  }

  /** Simulate daemon → panel frames. */
  push(...frames: unknown[]): void {
    this.sink?.(frames.map((frame) => JSON.stringify(frame)));
  }

  /** Simulate the wire dying. */
  drop(): void {
    this.down?.();
  }

  /** The single request frame sent so far matching `method`. */
  sentRequest(method: string, occurrence = 0): { id: number; params: any } {
    const matches = this.sent
      .map((frame) => JSON.parse(frame) as { id: number; method: string; params: any })
      .filter((message) => message.method === method);
    if (matches.length <= occurrence) {
      throw new Error(`no sent request ${method} #${occurrence}`);
    }
    return matches[occurrence]!;
  }
}

function rpcResult(id: number, result: unknown) {
  return { jsonrpc: "2.0", id, result };
}

function rpcError(id: number, error: unknown) {
  return { jsonrpc: "2.0", id, error };
}

const helloResult = {
  protocol: 1,
  protocolMin: 1,
  protocolMax: 1,
  daemon: { name: "zamind", version: "0.1.0" },
  capabilities: ["server.lifecycle", "streams"],
};

function notification(seq: number, payload: unknown, serverId = "alpha") {
  return {
    jsonrpc: "2.0",
    method: "streams.notification",
    params: { stream: "events", serverId, seq, payload },
  };
}

describe("ProtocolClient", () => {
  let transports: MockTransport[];

  function makeClient(options = {}) {
    const client = new ProtocolClient(
      async () => {
        const transport = new MockTransport();
        transports.push(transport);
        return transport;
      },
      { requestTimeoutMs: 1_000, backoffBaseMs: 1, backoffMaxMs: 2, ...options },
    );
    return client;
  }

  beforeEach(() => {
    transports = [];
    vi.useFakeTimers();
    // Deterministic backoff: no jitter.
    vi.spyOn(Math, "random").mockReturnValue(0);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  async function connectReady(): Promise<ProtocolClient> {
    const client = makeClient();
    const connecting = client.connect();
    await vi.advanceTimersByTimeAsync(0);
    const first = transports[0]!;
    first.push(rpcResult(first.sentRequest("daemon.hello").id, helloResult));
    await connecting;
    return client;
  }

  it("handshakes with the mandatory daemon.hello exchange and turns ready", async () => {
    const client = await connectReady();
    const hello = transports[0]!.sentRequest("daemon.hello");
    expect(hello.params.protocol).toBe(1);
    expect(hello.params.client.name).toBe("zim");
    expect(client.daemonInfo?.daemon.name).toBe("zamind");

    const statuses: string[] = [];
    client.onStatus((status) => statuses.push(status));
    expect(statuses[0]).toBe("ready");
    await client.dispose();
  });

  it("correlates replies by id and rejects with the structured error", async () => {
    const client = await connectReady();
    const transport = transports[0]!;

    const ok = client.request("server.list");
    const bad = client.request("server.start", { serverId: "alpha" });
    await vi.advanceTimersByTimeAsync(0);

    transport.push(rpcResult(transport.sentRequest("server.list").id, { servers: [] }));
    transport.push(
      rpcError(transport.sentRequest("server.start").id, {
        code: "SERVER_ALREADY_RUNNING",
        message: "The server is already running.",
        remediation: ["Open the console instead."],
      }),
    );
    await expect(ok).resolves.toEqual({ servers: [] });
    await expect(bad).rejects.toMatchObject({
      name: "ProtocolRequestError",
      error: { code: "SERVER_ALREADY_RUNNING" },
    });
    await client.dispose();
  });

  it("times a hung request out", async () => {
    const client = await connectReady();
    const pending = client.request("server.list");
    const expectation = expect(pending).rejects.toBeInstanceOf(RequestTimeoutError);
    await vi.advanceTimersByTimeAsync(1_001);
    await expectation;
    await client.dispose();
  });

  it("routes notifications to the handler registered before the reply", async () => {
    const client = await connectReady();
    const transport = transports[0]!;

    const payloads: any[] = [];
    const subscribed = client.subscribe("events", undefined, {
      onPayload: (n) => payloads.push(n),
    });
    await vi.advanceTimersByTimeAsync(0);

    // The opening replay batch can arrive before the subscribe reply.
    transport.push(notification(4, { kind: "event", event: { type: "jobStarted", job: {} } }));
    transport.push(
      rpcResult(transport.sentRequest("streams.subscribe").id, {
        subscriptionId: "sub-1",
        cursor: { seq: 4 },
        snapshot: { servers: [] },
      }),
    );
    await subscribed;

    expect(payloads).toHaveLength(1);
    expect(payloads[0]!.seq).toBe(4);

    transport.drop();
    await client.dispose();
  });

  it("rejects in-flight requests when the wire drops, then reconnects", async () => {
    const client = await connectReady();
    const transport = transports[0]!;

    const inFlight = client.request("server.list");
    const expectation = expect(inFlight).rejects.toBeInstanceOf(ConnectionLostError);
    transport.drop();
    await expectation;

    // The retry loop opens a second transport and handshakes it.
    await vi.advanceTimersByTimeAsync(10);
    expect(transports).toHaveLength(2);
    const second = transports[1]!;
    expect(second.started).toBe(1);
    second.push(rpcResult(second.sentRequest("daemon.hello").id, helloResult));
    await vi.advanceTimersByTimeAsync(0);
    expect(client.daemonInfo?.daemon.name).toBe("zamind");
    await client.dispose();
  });

  it("resumes the events stream from the last seen seq after reconnect", async () => {
    const client = await connectReady();
    const first = transports[0]!;

    const payloads: any[] = [];
    const registrations: any[] = [];
    const subscribed = client.subscribe("events", undefined, {
      onPayload: (n) => payloads.push(n),
      onRegistered: (result) => registrations.push(result),
    });
    await vi.advanceTimersByTimeAsync(0);
    first.push(
      rpcResult(first.sentRequest("streams.subscribe").id, {
        subscriptionId: "sub-1",
        snapshot: { servers: [] },
      }),
    );
    await subscribed;
    expect(registrations).toHaveLength(1); // first registration fires too

    first.push(notification(1, { kind: "event", event: { type: "jobStarted", job: {} } }));
    first.push(notification(2, { kind: "event", event: { type: "jobStarted", job: {} } }));
    first.push(notification(3, { kind: "event", event: { type: "jobStarted", job: {} } }));
    expect(payloads).toHaveLength(3);

    first.drop();
    await vi.advanceTimersByTimeAsync(10);
    const second = transports[1]!;
    second.push(rpcResult(second.sentRequest("daemon.hello").id, helloResult));
    await vi.advanceTimersByTimeAsync(0);

    const resub = second.sentRequest("streams.subscribe");
    expect(resub.params.cursor).toEqual({ seq: 3 });
    second.push(rpcResult(resub.id, { subscriptionId: "sub-2" }));
    await vi.advanceTimersByTimeAsync(0);
    expect(registrations).toHaveLength(2);
    await client.dispose();
  });

  it("falls back to a clean resubscribe when the cursor is too old", async () => {
    const client = await connectReady();
    const first = transports[0]!;

    const payloads: any[] = [];
    const registrations: any[] = [];
    const subscribed = client.subscribe("events", undefined, {
      onPayload: (n) => payloads.push(n),
      onRegistered: (result) => registrations.push(result),
    });
    await vi.advanceTimersByTimeAsync(0);
    first.push(
      rpcResult(first.sentRequest("streams.subscribe").id, {
        subscriptionId: "sub-1",
        cursor: { seq: 7 },
      }),
    );
    await subscribed;
    expect(registrations).toHaveLength(1); // first registration fires too
    registrations.length = 0;
    first.push(notification(8, { kind: "event", event: { type: "jobStarted", job: {} } }));

    first.drop();
    await vi.advanceTimersByTimeAsync(10);
    const second = transports[1]!;
    second.push(rpcResult(second.sentRequest("daemon.hello").id, helloResult));
    await vi.advanceTimersByTimeAsync(0);

    // First attempt: cursor {seq: 8} → rejected as too old.
    const stale = second.sentRequest("streams.subscribe");
    expect(stale.params.cursor).toEqual({ seq: 8 });
    second.push(rpcError(stale.id, { code: "PROTOCOL_INVALID_REQUEST", message: "cursor too old" }));
    await vi.advanceTimersByTimeAsync(0);

    // Fallback: no cursor, fresh snapshot delivered once.
    const clean = second.sentRequest("streams.subscribe", 1);
    expect(clean.params.cursor).toBeUndefined();
    const snapshotResult = {
      subscriptionId: "sub-2",
      cursorInvalid: true,
      snapshot: { servers: [{ serverId: "alpha", displayName: "Alpha", state: "stopped" }] },
    };
    second.push(rpcResult(clean.id, snapshotResult));
    await vi.advanceTimersByTimeAsync(0);

    expect(registrations).toHaveLength(1);
    expect(registrations[0]).toEqual(snapshotResult);
    await client.dispose();
  });

  it("falls back cleanly when a spec-shaped daemon answers cursorInvalid:true", async () => {
    const client = await connectReady();
    const first = transports[0]!;

    const registrations: any[] = [];
    const subscribed = client.subscribe("events", undefined, {
      onPayload: () => {},
      onRegistered: (result) => registrations.push(result),
    });
    await vi.advanceTimersByTimeAsync(0);
    first.push(
      rpcResult(first.sentRequest("streams.subscribe").id, {
        subscriptionId: "sub-1",
        cursor: { seq: 7 },
      }),
    );
    await subscribed;
    registrations.length = 0;

    first.drop();
    await vi.advanceTimersByTimeAsync(10);
    const second = transports[1]!;
    second.push(rpcResult(second.sentRequest("daemon.hello").id, helloResult));
    await vi.advanceTimersByTimeAsync(0);

    // Spec shape: a SUCCESS response that flags the cursor as unservable.
    // (No notification arrived since registration, so the last seen seq is
    // still 7 — the subscribe reply's cursor.)
    const stale = second.sentRequest("streams.subscribe");
    expect(stale.params.cursor).toEqual({ seq: 7 });
    second.push(rpcResult(stale.id, { subscriptionId: "sub-x", cursorInvalid: true }));
    await vi.advanceTimersByTimeAsync(0);

    const clean = second.sentRequest("streams.subscribe", 1);
    expect(clean.params.cursor).toBeUndefined();
    second.push(
      rpcResult(clean.id, {
        subscriptionId: "sub-2",
        snapshot: { servers: [] },
      }),
    );
    await vi.advanceTimersByTimeAsync(0);

    expect(registrations).toHaveLength(1);
    expect(registrations[0]!.snapshot).toEqual({ servers: [] });
    await client.dispose();
  });

  it("delivers Missed markers untouched", async () => {
    const client = await connectReady();
    const transport = transports[0]!;

    const payloads: any[] = [];
    const subscribed = client.subscribe("events", undefined, {
      onPayload: (n) => payloads.push(n),
    });
    await vi.advanceTimersByTimeAsync(0);
    transport.push(
      rpcResult(transport.sentRequest("streams.subscribe").id, { subscriptionId: "sub-1" }),
    );
    await subscribed;

    transport.push(notification(12, { kind: "missed", missed: 5 }));
    expect(payloads[0]!.payload).toEqual({ kind: "missed", missed: 5 });
    await client.dispose();
  });

  it("surfaces a failed transport as offline with retries scheduled", async () => {
    const client = makeClient();
    const statuses: string[] = [];
    client.onStatus((status) => statuses.push(status));

    void client.connect();
    // The first transport opens but the handshake hangs: the request times
    // out at 1000 ms, the failure is recorded, and retries kick in.
    await vi.advanceTimersByTimeAsync(1_100);

    expect(transports.length).toBeGreaterThanOrEqual(2);
    expect(statuses).toContain("offline");
    expect(statuses).toContain("connecting");
    await client.dispose();
  });

  it("sends hello.auth only when set, and the next handshake picks up changes", async () => {
    const client = makeClient();

    // Phase 1: no credential — local semantics, no auth field at all.
    void client.connect();
    await vi.advanceTimersByTimeAsync(0);
    const first = transports[0]!;
    const firstHello = first.sentRequest("daemon.hello");
    expect(firstHello.params).not.toHaveProperty("auth");
    first.push(rpcResult(firstHello.id, helloResult));

    // Phase 2: a credential is set; reconnect re-handshakes with it.
    client.setAuth("secret-token");
    const reconnected = client.reconnect();
    await vi.advanceTimersByTimeAsync(0);
    const second = transports[1]!;
    expect(first.stopped).toBe(1);
    const secondHello = second.sentRequest("daemon.hello");
    expect(secondHello.params.auth).toBe("secret-token");
    second.push(rpcResult(secondHello.id, helloResult));
    await reconnected;

    // Phase 3: cleared — the next handshake drops the field again.
    client.setAuth(undefined);
    const third = client.reconnect();
    await vi.advanceTimersByTimeAsync(0);
    const thirdTransport = transports[2]!;
    expect(thirdTransport.sentRequest("daemon.hello").params).not.toHaveProperty("auth");
    thirdTransport.push(
      rpcResult(thirdTransport.sentRequest("daemon.hello").id, helloResult),
    );
    await third;
    await client.dispose();
  });

  it("reconnect replaces the live transport without waiting for the retry schedule", async () => {
    const client = makeClient();
    void client.connect();
    await vi.advanceTimersByTimeAsync(0);
    const first = transports[0]!;
    first.push(rpcResult(first.sentRequest("daemon.hello").id, helloResult));

    const reconnected = client.reconnect();
    await vi.advanceTimersByTimeAsync(0);
    const second = transports[1]!;
    second.push(rpcResult(second.sentRequest("daemon.hello").id, helloResult));
    await reconnected;
    expect(transports.length).toBe(2);
    expect(first.stopped).toBe(1);
    await client.dispose();
  });
});

describe("ProtocolRequestError", () => {
  it("carries the structured error for UI rendering", () => {
    const error = new ProtocolRequestError({
      code: "FS_NOT_FOUND",
      message: "The server directory does not exist.",
      remediation: ["Check the path."],
    });
    expect(error.error.code).toBe("FS_NOT_FOUND");
    expect(error.message).toBe("The server directory does not exist.");
  });
});
