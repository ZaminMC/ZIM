// Transport seam (ADR-0003): the protocol client is transport-agnostic; a
// transport moves JSON-RPC frames. Incoming data always arrives as a batch
// of frame strings — the Tauri host coalesces high-frequency notifications
// (~50 ms, PERFORMANCE-BUDGETS) — so single-frame transports wrap once.

export type IncomingBatch = readonly string[];

export interface Transport {
  /** Open the transport. `sink` receives incoming frame batches; `onDown`
   *  fires once when the underlying connection is lost. */
  start(sink: (batch: IncomingBatch) => void, onDown: () => void): Promise<void>;
  /** Send one JSON-RPC frame (serialized). */
  send(frame: string): void;
  /** Close the transport cleanly. */
  stop(): Promise<void>;
  /** The wedged-daemon ask: the host probes whether the daemon ANSWERS
   *  (a corpse holds the pipe while its session never speaks), ends the
   *  stale image, and respawns the sibling. Local transports implement
   *  it; a remote wire has nobody to heal from here and answers false.
   *  Optional — the demo's stand-in transport has no host behind it. */
  ensure?(): Promise<boolean>;
}

// --- WebSocket transport (development: the Node dev bridge) ---

export class WsTransport implements Transport {
  private socket: WebSocket | null = null;

  constructor(private readonly url: string) {}

  start(sink: (batch: IncomingBatch) => void, onDown: () => void): Promise<void> {
    return new Promise((resolve, reject) => {
      const socket = new WebSocket(this.url);
      this.socket = socket;
      socket.addEventListener("open", () => resolve());
      socket.addEventListener("error", () => {
        if (socket.readyState === WebSocket.CONNECTING) {
          reject(new Error(`could not reach the dev bridge at ${this.url}`));
        }
      });
      socket.addEventListener("message", (event) => {
        if (typeof event.data === "string") sink([event.data]);
      });
      socket.addEventListener("close", () => onDown());
    });
  }

  send(frame: string): void {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      throw new Error("transport is not open");
    }
    socket.send(frame);
  }

  stop(): Promise<void> {
    const socket = this.socket;
    this.socket = null;
    if (socket && socket.readyState !== WebSocket.CLOSED) {
      socket.close();
    }
    return Promise.resolve();
  }
}

// --- Tauri transport (production: the thin Rust host bridge) ---
//
// The host owns the daemon connection and forwards frames; all logic
// (handshake, correlation, reconnect, cursors) stays in this webview. The
// `@tauri-apps/api` import is dynamic so browser builds and tests never
// load it.
//
// A non-null `remote` points the host at a zaminagent over TLS (ADR-0011):
// the host dials the relay with the fingerprint pinned; the token still
// travels only inside the tunnel, as hello.auth from this webview. A
// remote wire has no `daemon_ensure` safety net — a remote box's daemon
// is nobody's spawn target from here.

export interface TauriRemoteSpec {
  addr: string;
  token: string;
  fingerprint: string;
}

export class TauriTransport implements Transport {
  constructor(private readonly remote: TauriRemoteSpec | null = null) {}

  async start(sink: (batch: IncomingBatch) => void, onDown: () => void): Promise<void> {
    const { invoke, Channel } = await import("@tauri-apps/api/core");

    const attempt = async (): Promise<void> => {
      // Fresh channels per attempt: a failed daemon_connect must never leave
      // half-registered channels behind for the retry to trip over.
      const frames = new Channel<string>((message) => {
        // The host sends each batch as a JSON array of frame strings.
        try {
          const parsed: unknown = JSON.parse(message);
          if (Array.isArray(parsed)) {
            sink(parsed.filter((frame): frame is string => typeof frame === "string"));
          }
        } catch {
          // Unparseable host message: drop it, the reader stays alive.
        }
      });
      const down = new Channel<null>(() => onDown());
      await invoke("daemon_connect", {
        frames,
        down,
        remoteAddr: this.remote?.addr ?? null,
        remoteToken: this.remote?.token ?? null,
        remoteFingerprint: this.remote?.fingerprint ?? null,
      });
    };

    try {
      await attempt();
    } catch (error) {
      // Local wire down: first contact or the daemon died mid-session.
      // §1.2 — double-clicking the panel must never show a daemon error,
      // so the host brings the daemon back (probe → spawn sibling → wait
      // for bind) before the client's retry schedule gets its next turn.
      // A remote wire gets the honest failure instead: the box's daemon is
      // out of reach from here, spawning one would be a lie.
      if (this.remote) throw error;
      const outcome = await invoke<string>("daemon_ensure").catch(() => null);
      if (outcome !== "spawned" && outcome !== "already-running") {
        throw error; // original failure is the honest one (e.g. no binary)
      }
      await attempt();
    }
  }

  send(frame: string): void {
    void (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("daemon_send", { frame });
    })();
  }

  async ensure(): Promise<boolean> {
    const { invoke } = await import("@tauri-apps/api/core");
    const outcome = await invoke<string>("daemon_ensure").catch(() => null);
    return outcome === "spawned" || outcome === "already-running";
  }

  async stop(): Promise<void> {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("daemon_close");
  }
}
