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

export class TauriTransport implements Transport {
  async start(sink: (batch: IncomingBatch) => void, onDown: () => void): Promise<void> {
    const { invoke, Channel } = await import("@tauri-apps/api/core");

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

    await invoke("daemon_connect", { frames, down });
  }

  send(frame: string): void {
    void (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("daemon_send", { frame });
    })();
  }

  async stop(): Promise<void> {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("daemon_close");
  }
}
