#!/usr/bin/env node
// Development bridge: browser WebSocket ⇄ zamind's framed Unix-socket
// transport. A dev tool only — the shipped product goes through the Tauri
// host bridge (ADR-0003: no Node in the system side).
//
// Frame format (zamin-protocol framing.rs): u32 little-endian byte count +
// payload, capped at 16 MiB.
//
// Local mode (default): every session relays to the local daemon socket.
// Remote mode (ADR-0011): a session whose URL carries
//   ?remote=<host:port>&fingerprint=<sha256-hex>
// relays instead over TLS to a zaminagent. The fingerprint pins the
// agent's self-signed certificate; omitting it accepts any certificate
// (the documented skip-verify escape hatch — the bridge logs a warning).
// The token never crosses the bridge: the panel sends it as hello.auth
// inside the TLS tunnel.
//
// Usage: node dev-bridge.mjs [--endpoint /path/to/zamind.sock] [--port 8787]

import os from "node:os";
import path from "node:path";
import net from "node:net";
import tls from "node:tls";
import { WebSocketServer } from "ws";

const MAX_FRAME_LENGTH = 16 * 1024 * 1024;

function parseArgs(argv) {
  const args = { port: 8787, endpoint: null };
  for (let i = 2; i < argv.length; i += 1) {
    const value = argv[i + 1];
    if (argv[i] === "--port" && value) {
      args.port = Number.parseInt(value, 10);
      i += 1;
    } else if (argv[i] === "--endpoint" && value) {
      args.endpoint = value;
      i += 1;
    }
  }
  return args;
}

// Mirrors zamin-ipc Endpoint::default_endpoint (Unix lane).
function defaultEndpoint() {
  const runtimeDir =
    process.env.XDG_RUNTIME_DIR && process.env.XDG_RUNTIME_DIR.length > 0
      ? process.env.XDG_RUNTIME_DIR
      : path.join(os.tmpdir(), `zamind-runtime-${process.getuid?.() ?? 0}`);
  return path.join(runtimeDir, "zamind", "zamind.sock");
}

// ADR-0011: the session target comes from the WebSocket URL, so one bridge
// serves the local fleet and any number of remote agents side by side.
function parseTarget(url) {
  const parsed = new URL(url, "http://localhost");
  const remote = parsed.searchParams.get("remote");
  if (!remote) return { kind: "local" };
  const [host, portStr] = remote.split(":");
  const fingerprint = (parsed.searchParams.get("fingerprint") ?? "")
    .replace(/:/g, "")
    .toLowerCase();
  return {
    kind: "remote",
    host,
    port: Number.parseInt(portStr, 10) || 7443,
    fingerprint,
  };
}

function connectTarget(target) {
  if (target.kind === "local") return net.createConnection(endpoint);
  if (!target.fingerprint) {
    console.warn(
      `[dev-bridge] remote ${target.host}:${target.port} without a pinned fingerprint — ` +
        "accepting any server certificate (insecure mode)",
    );
  }
  return tls.connect({
    host: target.host,
    port: target.port,
    // The pin (when present) IS the verification; no CA, no trust store.
    rejectUnauthorized: false,
    checkServerIdentity: (hostname, cert) => {
      if (!target.fingerprint) return undefined;
      const actual = (cert.fingerprint256 ?? "").replace(/:/g, "").toLowerCase();
      if (actual !== target.fingerprint) {
        return new Error(
          `agent fingerprint ${actual} does not match the pinned ${target.fingerprint}`,
        );
      }
      return undefined;
    },
  });
}

function encodeFrame(payload) {
  const head = Buffer.alloc(4);
  head.writeUInt32LE(payload.length, 0);
  return Buffer.concat([head, payload]);
}

class FrameDecoder {
  constructor() {
    this.buffer = Buffer.alloc(0);
  }

  push(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    const frames = [];
    for (;;) {
      if (this.buffer.length < 4) break;
      const length = this.buffer.readUInt32LE(0);
      if (length > MAX_FRAME_LENGTH) {
        throw new Error(`frame of ${length} bytes exceeds the ${MAX_FRAME_LENGTH}-byte limit`);
      }
      if (this.buffer.length < 4 + length) break;
      frames.push(this.buffer.subarray(4, 4 + length).toString("utf8"));
      this.buffer = this.buffer.subarray(4 + length);
    }
    return frames;
  }
}

const args = parseArgs(process.argv);
const endpoint = args.endpoint ?? defaultEndpoint();

const wss = new WebSocketServer({ host: "127.0.0.1", port: args.port }, () => {
  console.log(`[dev-bridge] listening on ws://127.0.0.1:${args.port}`);
  console.log(`[dev-bridge] daemon endpoint: ${endpoint}`);
  console.log("[dev-bridge] remote sessions: ws url ?remote=<host:port>&fingerprint=<sha256>");
});

wss.on("error", (error) => {
  console.error(`[dev-bridge] ${error.message}`);
  process.exitCode = 1;
});

wss.on("connection", (ws, req) => {
  const target = parseTarget(req.url ?? "/");
  const label =
    target.kind === "remote" ? `agent ${target.host}:${target.port}` : `daemon ${endpoint}`;
  const socket = connectTarget(target);
  const decoder = new FrameDecoder();
  let wireDead = false;

  const kill = () => {
    if (wireDead) return;
    wireDead = true;
    socket.destroy();
    ws.terminate();
  };

  socket.on("connect", () => {
    console.log(`[dev-bridge] browser session connected to the ${label}`);
  });

  socket.on("data", (chunk) => {
    let frames;
    try {
      frames = decoder.push(chunk);
    } catch (error) {
      console.error(`[dev-bridge] ${error.message}`);
      kill();
      return;
    }
    for (const frame of frames) {
      if (ws.readyState === ws.OPEN) ws.send(frame);
    }
  });

  socket.on("close", kill);
  socket.on("error", (error) => {
    console.error(`[dev-bridge] ${label}: ${error.message}`);
    kill();
  });

  ws.on("message", (data) => {
    if (wireDead) return;
    socket.write(encodeFrame(Buffer.from(data)));
  });

  ws.on("close", kill);
  ws.on("error", kill);
});
