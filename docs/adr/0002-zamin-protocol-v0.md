# ADR-0002 — Zamin Protocol v0

**Status:** Accepted · **Date:** 2026-10-05

## Context

Panel, CLI, and a future remote agent need one boundary to ZaminCore. If clients ever touch Core's internal API, the remote architecture quietly dies. The protocol must be fixed at v0 with the shapes that are expensive to retrofit: errors, jobs, subscriptions, cursors, chunked file transfer, and a handshake.

## Decision

Adopt **Zamin Protocol v0** as specified in [docs/architecture/protocol-v0.md](../architecture/protocol-v0.md). Decision-level rules:

1. **JSON-RPC 2.0** with u32 length-prefixed UTF-8 JSON framing over a byte stream (named pipe / Unix socket). One multiplexed connection carries requests, notifications, and streams. No message batching in v0.
2. **Versioned handshake** (`daemon.hello`) is mandatory and first. The protocol version is an integer starting at 1. Additive changes do not bump it; breaking changes bump it and define a min/max negotiation.
3. **Tolerant readers.** Clients and daemon ignore unknown fields. This rule is what makes additive evolution free.
4. **Typed errors**: stable string codes (`PORT_IN_USE`), a human message, structured context, and remediation action IDs. Codes are registered in `zamin-protocol` as an enum.
5. **Servers are referenced by `serverId`**, never by filesystem paths. Paths appear only server-root-relative, and only in file operations.
6. **Idempotent lifecycle commands.** Mutating requests carry a client-generated request ID (UUIDv7). The daemon deduplicates against in-flight and recently completed requests (bounded, TTL'd).
7. **Jobs are first-class** (`job.started / job.progress / job.completed{outcome}`) with cancellation via `job.cancel`.
8. **Streams and cursors**: subscriptions carry monotonic sequence numbers; reconnect = snapshot + replay-from-cursor, with per-stream-type semantics (ADR-0006).
9. **Timestamps** are Unix epoch milliseconds with a `_ms` suffix on every field.
10. **Auth hook**: `daemon.hello` carries an opaque `auth` value. Local transports ignore it; a remote transport will require it. Nothing more is built now.

The v0 method catalog is minimal (daemon, server lifecycle, streams, jobs). File-transfer messages are specified in v0 but implemented with the file manager.

## Consequences

- `zamin-protocol` (types, no I/O) and `zamin-ipc` (framing + transport, client and server) are separate crates; clients depend on both, never on `zamin-core`.
- Protocol conformance fixtures live in CI from the first release.

## Alternatives considered

- gRPC/protobuf: codegen and toolchain weight; framed JSON-RPC is debuggable by hand and sufficient at our message rates.
- WebSocket-first: adds a transport concern before the message model is proven.
