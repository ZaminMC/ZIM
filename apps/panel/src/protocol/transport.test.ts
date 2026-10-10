// Transport seam tests (ADR-0003): the two liveness laws that were only
// prose before —
//   1. WsTransport.start() settles even when stop() closes a CONNECTING
//      socket (the dev wire used to hang forever: no retry, no down).
//   2. TauriTransport.send() reports a failed daemon_send through the
//      down lane (a dropped frame used to stall its caller for the full
//      request budget before anyone learned).

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { waitFor } from "@testing-library/react";
import { WsTransport, TauriTransport } from "./transport";

// The host bridge's seam, hoisted so vi.mock's factory can hand the
// dynamic import the very mocks these tests drive — without importing
// the restricted module directly (only the integration seam may).
const core = vi.hoisted(() => ({
  invoke: vi.fn(),
  Channel: class<T> {
    constructor(public readonly onMessage: (message: T) => void) {}
  },
}));
vi.mock("@tauri-apps/api/core", () => core);

/** A controllable WebSocket stand-in: starts CONNECTING, opens and closes
 *  on demand, records sent frames. */
class FakeSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  readyState = FakeSocket.CONNECTING;
  sent: string[] = [];
  private readonly listeners = new Map<string, Array<(event: { data?: string }) => void>>();

  addEventListener(type: string, fn: (event: { data?: string }) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), fn]);
  }

  removeEventListener(): void {}

  private dispatch(type: string, event: { data?: string } = {}): void {
    for (const fn of this.listeners.get(type) ?? []) fn(event);
  }

  open(): void {
    this.readyState = FakeSocket.OPEN;
    this.dispatch("open");
  }

  message(data: string): void {
    this.dispatch("message", { data });
  }

  close(): void {
    if (this.readyState === FakeSocket.CLOSED) return;
    this.readyState = FakeSocket.CLOSED;
    this.dispatch("close");
  }

  error(): void {
    this.dispatch("error");
  }

  send(frame: string): void {
    this.sent.push(frame);
  }
}

describe("WsTransport — the dev bridge wire", () => {
  let socket: FakeSocket;

  beforeEach(() => {
    socket = new FakeSocket();
    vi.stubGlobal(
      "WebSocket",
      class {
        static readonly CONNECTING = 0;
        static readonly OPEN = 1;
        static readonly CLOSING = 2;
        static readonly CLOSED = 3;
        constructor() {
          return socket;
        }
      },
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("frames arrive as one-frame batches once open", async () => {
    const transport = new WsTransport("ws://bridge");
    const batches: string[][] = [];
    const started = transport.start((batch) => batches.push([...batch]), () => {});
    socket.open();
    await expect(started).resolves.toBeUndefined();
    socket.message('{"jsonrpc":"2.0"}');
    expect(batches).toEqual([['{"jsonrpc":"2.0"}']]);
    transport.send('{"id":1}');
    expect(socket.sent).toEqual(['{"id":1}']);
  });

  it("a stop while still connecting settles the start promise — no silent hang", async () => {
    const transport = new WsTransport("ws://bridge");
    const started = transport.start(() => {}, () => {});
    // The wire is torn down mid-handshake: the socket closes without
    // ever opening. The promise MUST settle (reject), or the client's
    // connectOnce awaits forever.
    void transport.stop();
    await expect(started).rejects.toThrow(/closed before it opened/);
  });

  it("a failed connect rejects with the bridge's own words and still speaks down", async () => {
    const onDown = vi.fn();
    const transport = new WsTransport("ws://bridge");
    const started = transport.start(() => {}, onDown);
    socket.error();
    socket.close();
    await expect(started).rejects.toThrow(/could not reach the dev bridge/);
    expect(onDown).toHaveBeenCalled();
  });
});

describe("TauriTransport — the host bridge", () => {
  beforeEach(() => {
    core.invoke.mockReset();
  });

  it("a failed daemon_send speaks the down lane instead of stalling its caller", async () => {
    core.invoke.mockImplementation((cmd: string) => {
      if (cmd === "daemon_send") return Promise.reject(new Error("the host wire is gone"));
      return Promise.resolve();
    });
    const onDown = vi.fn();
    const transport = new TauriTransport(null);
    await transport.start(() => {}, onDown);
    transport.send('{"jsonrpc":"2.0","method":"daemon.ping","id":1}');
    await waitFor(() => expect(onDown).toHaveBeenCalled());
    // The client's generation guard absorbs the echo of the host's own
    // down channel — the transport's job is only to speak once, early.
    expect(onDown).toHaveBeenCalledTimes(1);
  });

  it("a healthy send stays silent on the down lane", async () => {
    core.invoke.mockResolvedValue(undefined);
    const onDown = vi.fn();
    const transport = new TauriTransport(null);
    await transport.start(() => {}, onDown);
    transport.send('{"jsonrpc":"2.0","method":"daemon.ping","id":1}');
    // Give the async send a turn to prove it did not speak.
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(onDown).not.toHaveBeenCalled();
  });
});
