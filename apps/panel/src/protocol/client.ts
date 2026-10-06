// The panel's protocol client (ADR-0003): a plain module — not a framework
// object — that owns the handshake, request correlation, stream
// subscriptions, and reconnection with cursors. This is the only place
// reconnection is implemented (STYLE-GUIDE, TypeScript).
//
// Wire contract: zamin-protocol v1 (protocol spec §1–§6). The daemon's
// error responses carry the structured error object directly in `error`.

import type {
  HelloResult,
  ProtocolErrorObject,
  StreamCursor,
  StreamKind,
  StreamNotification,
  SubscribeParams,
  SubscribeResult,
} from "./types";
import { PROTOCOL_VERSION } from "./types";
import type { IncomingBatch, Transport } from "./transport";

export const CLIENT_NAME = "zamin-panel";
export const CLIENT_VERSION = "0.1.0";

export type ClientStatus = "offline" | "connecting" | "ready";

export class ProtocolRequestError extends Error {
  constructor(public readonly error: ProtocolErrorObject) {
    super(error.message);
    this.name = "ProtocolRequestError";
  }
}

export class RequestTimeoutError extends Error {
  constructor(method: string, ms: number) {
    super(`the daemon did not answer ${method} within ${ms} ms`);
    this.name = "RequestTimeoutError";
  }
}

export class ConnectionLostError extends Error {
  constructor() {
    super("the connection to the daemon was lost before a reply arrived");
    this.name = "ConnectionLostError";
  }
}

export class DisposedError extends Error {
  constructor() {
    super("the client was disposed");
    this.name = "DisposedError";
  }
}

export interface StreamHandler {
  /** Every notification on this subscription's stream. */
  onPayload(notification: StreamNotification): void;
  /** Fired once the subscription is registered on the daemon — including
   *  the first time. A fresh `snapshot` (when present) must be reconciled;
   *  `cursorInvalid` means the previously requested cursor was too old and
   *  this result is a clean re-registration (ADR-0006). */
  onRegistered?(result: SubscribeResult): void;
}

export interface SubscriptionHandle {
  dispose(): void;
}

interface ActiveSubscription {
  stream: StreamKind;
  serverId?: string;
  handler: StreamHandler;
  subscriptionId: string | null;
  /** Last `seq` seen on the events stream; resumes exactly here. */
  lastSeq: number | null;
  /** Set when dispose() ran before the subscribe reply arrived. */
  disposeAfterReply: boolean;
  /** Explicit first-subscribe cursor (e.g. a logs subscription that must
   *  start live-only instead of replaying the ring). Also rides every
   *  resubscribe: `resumeCursorFor` has nothing for logs streams, so the
   *  initial choice (replay or not) is sticky across reconnects. */
  initialCursor?: StreamCursor;
}

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
  timer: ReturnType<typeof setTimeout>;
}

export interface ClientOptions {
  /** Per-request timeout. Default: 10 s (mirrors the CLI). */
  requestTimeoutMs?: number;
  /** Reconnect backoff. Default: 250 ms base, doubling to a 5 s cap,
   *  plus jitter. */
  backoffBaseMs?: number;
  backoffMaxMs?: number;
}

const defaultTimeoutMs = 10_000;
const defaultBackoffBaseMs = 250;
const defaultBackoffMaxMs = 5_000;

export class ProtocolClient {
  private nextRequestId = 1;
  private readonly pending = new Map<number, PendingRequest>();
  private readonly subs = new Map<symbol, ActiveSubscription>();
  private readonly statusListeners = new Set<(status: ClientStatus) => void>();

  private generation = 0;
  private retryAttempt = 0;
  private disposed = false;
  private transport: Transport | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private lastFailure: unknown = null;
  private hello: HelloResult | null = null;
  private status: ClientStatus = "offline";
  /** Credential for the next handshake (remote transport, ADR-0011). */
  private auth: string | undefined;

  constructor(
    private readonly createTransport: () => Promise<Transport>,
    private readonly options: ClientOptions = {},
  ) {}

  private get timeoutMs(): number {
    return this.options.requestTimeoutMs ?? defaultTimeoutMs;
  }

  // --- lifecycle ---

  /** Open the transport, run the mandatory handshake (§2), resubscribe any
   *  streams registered earlier. Errors during connect are swallowed into
   *  the retry schedule; observe `status` for progress. */
  async connect(): Promise<void> {
    await this.connectOnce();
  }

  /** Drop the current connection (and any pending retry) and open a fresh
   *  one — the operator switched connection profiles (ADR-0011). The new
   *  handshake picks up the credential set via `setAuth`. */
  async reconnect(): Promise<void> {
    if (this.disposed) return;
    if (this.retryTimer !== null) {
      clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
    const transport = this.transport;
    if (transport) {
      this.generation += 1; // stale-guard everything still in flight
      this.hello = null;
      this.transport = null;
      for (const sub of this.subs.values()) {
        sub.subscriptionId = null;
      }
      this.rejectAllPending(new ConnectionLostError());
      this.retryAttempt = 0;
      await transport.stop().catch(() => {});
      this.setStatus("offline");
    }
    await this.connect();
  }

  /** Tear everything down: no reconnects, no pending replies. */
  async dispose(): Promise<void> {
    this.disposed = true;
    this.generation += 1;
    if (this.retryTimer !== null) {
      clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
    for (const sub of this.subs.values()) {
      sub.handler.onRegistered = undefined;
    }
    this.rejectAllPending(new DisposedError());
    const transport = this.transport;
    this.transport = null;
    if (transport) {
      await transport.stop().catch(() => {});
    }
    this.setStatus("offline");
  }

  onStatus(listener: (status: ClientStatus) => void): () => void {
    this.statusListeners.add(listener);
    listener(this.status);
    return () => this.statusListeners.delete(listener);
  }

  /** The daemon's hello reply once `ready`; null before that. */
  get daemonInfo(): HelloResult | null {
    return this.hello;
  }

  /** The last transport-level failure, for diagnostics surfaces. */
  get lastError(): unknown {
    return this.lastFailure;
  }

  /** Set the hello `auth` credential, read by the next handshake — i.e. by
   *  `connect()` / `reconnect()`. `undefined` means local semantics (the
   *  daemon ignores auth on local transports). */
  setAuth(auth: string | undefined): void {
    this.auth = auth;
  }

  private setStatus(status: ClientStatus): void {
    this.status = status;
    for (const listener of this.statusListeners) listener(status);
  }

  // --- connection / reconnection ---

  private async connectOnce(): Promise<void> {
    const generation = ++this.generation;
    this.setStatus("connecting");
    try {
      const transport = await this.createTransport();
      if (this.isStale(generation)) {
        await transport.stop().catch(() => {});
        return;
      }
      await transport.start(
        (batch) => this.handleBatch(batch, generation),
        () => this.handleDown(generation),
      );
      if (this.isStale(generation)) {
        await transport.stop().catch(() => {});
        return;
      }
      this.transport = transport;
      this.hello = await this.helloExchange();
      if (this.isStale(generation)) return;
      this.retryAttempt = 0;
      this.setStatus("ready");
      await this.resubscribeAll(generation);
    } catch (error) {
      if (this.isStale(generation)) return;
      this.lastFailure = error;
      await this.transport?.stop().catch(() => {});
      this.transport = null;
      this.setStatus("offline");
      this.scheduleRetry();
    }
  }

  private isStale(generation: number): boolean {
    return this.disposed || generation !== this.generation;
  }

  private handleDown(generation: number): void {
    if (this.isStale(generation)) return;
    this.generation += 1;
    this.hello = null;
    this.transport = null;
    for (const sub of this.subs.values()) {
      sub.subscriptionId = null;
    }
    this.rejectAllPending(new ConnectionLostError());
    this.setStatus("offline");
    this.scheduleRetry();
  }

  private scheduleRetry(): void {
    if (this.disposed) return;
    const base = this.options.backoffBaseMs ?? defaultBackoffBaseMs;
    const max = this.options.backoffMaxMs ?? defaultBackoffMaxMs;
    const delay = Math.min(max, base * 2 ** this.retryAttempt) + Math.random() * 100;
    this.retryAttempt += 1;
    const generation = this.generation;
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      if (!this.isStale(generation)) void this.connectOnce();
    }, delay);
  }

  // --- requests ---

  async request<R = unknown>(method: string, params?: unknown): Promise<R> {
    // Guard on the live transport, not on `ready`: the handshake itself is
    // a request issued between transport-start and ready.
    if (!this.transport) {
      throw new ConnectionLostError();
    }
    const id = this.nextRequestId;
    this.nextRequestId += 1;
    const frame = {
      jsonrpc: "2.0",
      id,
      method,
      ...(params === undefined ? {} : { params }),
    };

    const reply = this.waitForReply(id, method);
    this.transport.send(JSON.stringify(frame));
    return (await reply) as R;
  }

  private waitForReply(id: number, method: string): Promise<unknown> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new RequestTimeoutError(method, this.timeoutMs));
      }, this.timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
    });
  }

  private rejectAllPending(error: unknown): void {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
  }

  private async helloExchange(): Promise<HelloResult> {
    return this.request<HelloResult>("daemon.hello", {
      protocol: PROTOCOL_VERSION,
      ...(this.auth === undefined ? {} : { auth: this.auth }),
      client: { name: CLIENT_NAME, version: CLIENT_VERSION },
    });
  }

  // --- streams ---

  /** Subscribe to a stream. The handler is registered *before* the request
   *  goes out, so the opening replay batch can never race the reply.
   *  `initialCursor` pins the first subscribe's cursor (and every later
   *  resubscribe's, for streams without a resume cursor): a logs
   *  subscription that must start live-only passes a logs cursor and the
   *  daemon skips its ring replay. */
  async subscribe(
    stream: StreamKind,
    serverId: string | undefined,
    handler: StreamHandler,
    initialCursor?: StreamCursor,
  ): Promise<SubscriptionHandle> {
    const key = Symbol("subscription");
    const sub: ActiveSubscription = {
      stream,
      serverId,
      handler,
      subscriptionId: null,
      lastSeq: null,
      disposeAfterReply: false,
      ...(initialCursor === undefined ? {} : { initialCursor }),
    };
    this.subs.set(key, sub);

    if (this.status === "ready") {
      await this.sendSubscribe(sub);
    }
    return {
      dispose: () => {
        const current = this.subs.get(key);
        if (!current) return;
        if (current.subscriptionId === null) {
          // Reply not in yet: unsubscribe as soon as it lands.
          current.disposeAfterReply = true;
        } else {
          void this.unsubscribeQuietly(current.subscriptionId);
          this.subs.delete(key);
        }
      },
    };
  }

  private async sendSubscribe(sub: ActiveSubscription, allowFallback = true): Promise<void> {
    const cursor = this.resumeCursorFor(sub) ?? sub.initialCursor;
    const params: SubscribeParams = {
      stream: sub.stream,
      ...(sub.serverId === undefined ? {} : { serverId: sub.serverId }),
      ...(cursor === undefined ? {} : { cursor }),
    };
    let result: SubscribeResult;
    try {
      result = await this.request<SubscribeResult>("streams.subscribe", params);
    } catch (error) {
      // A cursor the replay ring can no longer serve means "re-snapshot"
      // (ADR-0006). Spec-shaped daemons answer success with
      // `cursorInvalid: true`; others answer a structured error. Either
      // way: drop the cursor and resubscribe cleanly, exactly once.
      if (cursor !== undefined && allowFallback && error instanceof ProtocolRequestError) {
        sub.lastSeq = null;
        return this.sendSubscribe(sub, false);
      }
      throw error;
    }
    if (result.cursorInvalid && cursor !== undefined && allowFallback) {
      sub.lastSeq = null;
      return this.sendSubscribe(sub, false);
    }
    sub.subscriptionId = result.subscriptionId;
    if (sub.lastSeq === null) {
      sub.lastSeq = result.cursor && "seq" in result.cursor ? result.cursor.seq : null;
    }
    if (sub.disposeAfterReply) {
      void this.unsubscribeQuietly(result.subscriptionId);
      for (const [key, candidate] of this.subs) {
        if (candidate === sub) this.subs.delete(key);
      }
      return;
    }
    sub.handler.onRegistered?.(result);
  }

  private resumeCursorFor(sub: ActiveSubscription): StreamCursor | undefined {
    // Events resume exactly from the last seen seq. Logs resume without a
    // cursor in v1: notifications carry no file offset yet, so a stale
    // cursor would replay duplicate lines into the terminal (documented
    // gap; the console marks the seam instead).
    if (sub.stream === "events" && sub.lastSeq !== null) {
      return { seq: sub.lastSeq };
    }
    return undefined;
  }

  private async resubscribeAll(generation: number): Promise<void> {
    for (const sub of [...this.subs.values()]) {
      if (this.isStale(generation)) return;
      try {
        await this.sendSubscribe(sub);
      } catch (error) {
        if (this.isStale(generation)) return;
        // A failed resubscribe keeps the handler registered; the next
        // successful (re)connect tries again. Surface via console in dev.
        this.lastFailure = error;
      }
    }
  }

  private async unsubscribeQuietly(subscriptionId: string): Promise<void> {
    try {
      await this.request("streams.unsubscribe", { subscriptionId });
    } catch {
      // Best effort; the daemon drops subscriptions when the wire drops.
    }
  }

  // --- incoming frames ---

  private handleBatch(batch: IncomingBatch, generation: number): void {
    for (const frame of batch) this.handleFrame(frame, generation);
  }

  private handleFrame(frame: string, generation: number): void {
    let parsed: unknown;
    try {
      parsed = JSON.parse(frame);
    } catch {
      return; // Tolerant reader: skip unparseable frames.
    }
    if (typeof parsed !== "object" || parsed === null) return;
    const message = parsed as Record<string, unknown>;

    if ("method" in message) {
      if ("id" in message) return; // Server-originated requests: none in v1.
      this.handleNotification(message);
      return;
    }
    if (!("id" in message)) return;
    const pending = this.pending.get(message.id as number);
    if (!pending) return; // Unknown or timed-out id: drop.
    this.pending.delete(message.id as number);
    clearTimeout(pending.timer);
    if (generation !== this.generation) return; // Reply from a dead transport.

    const error = message.error as ProtocolErrorObject | undefined;
    if (error) {
      pending.reject(new ProtocolRequestError(error));
    } else {
      pending.resolve(message.result);
    }
  }

  private handleNotification(message: Record<string, unknown>): void {
    if (message.method !== "streams.notification") return;
    const params = message.params as StreamNotification | undefined;
    if (!params || typeof params.seq !== "number") return;

    for (const sub of this.subs.values()) {
      if (sub.subscriptionId !== null && sub.disposeAfterReply) continue;
      const targeted = sub.serverId === undefined || sub.serverId === params.serverId;
      if (!targeted || sub.stream !== params.stream) continue;
      if (params.stream === "events" && params.seq > (sub.lastSeq ?? 0)) {
        sub.lastSeq = params.seq;
      }
      sub.handler.onPayload(params);
    }
  }
}
