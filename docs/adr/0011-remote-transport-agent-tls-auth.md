# ADR-0011 — Remote transport: ZaminAgent with TLS and token auth

**Status:** Accepted · **Date:** 2026-10-06

## Context

Phase 8 (Services & remote) calls for a headless box managed from a desktop Panel. ADR-0001 reserved the shape years ahead of the code: the daemon stays local, and "ZaminPanel, ZaminCLI, and the future ZaminAgent are protocol clients." The protocol shipped with the hook already in place — `daemon.hello` carries an `auth` field that local transports ignore, and framing is transport-agnostic. What had to be decided now is where the network boundary lives, what TLS looks like without a CA infrastructure, and what the credential is.

The constraints that matter:

- The daemon is a per-user local process. Making it network-facing changes its security posture, its single-instance model, and its blast radius in one stroke.
- A home-lab user has no CA, no domain, and no appetite for certificate management. But they do have SSH and a text editor.
- The proof of done is a real desktop Panel operating a daemon it cannot reach by socket.

## Decision

### The agent is the network-facing process; the daemon never is

`zaminagent` is a new protocol client that runs on the machine hosting the daemon. It listens on TCP, terminates TLS, authenticates the remote client, and relays frames to the local daemon. Per remote connection it opens exactly one local daemon session — the daemon's per-session model, request dedupe, and subscription semantics are untouched. The agent is deliberately shallow: it understands frames, the first exchange, and nothing else. No server, job, or file semantics live in it, so protocol evolution cannot fork the agent.

The agent requires an already-running daemon and never spawns one: on a headless host the daemon's service unit owns its lifetime (the systemd user unit from the previous slice). An unreachable daemon is a typed answer (`DAEMON_UNREACHABLE`), not a spawn side effect.

### TLS without a CA: one self-signed certificate, pinned by fingerprint

On first start the agent generates a self-signed certificate (CN `zaminagent`, ring-backed, TLS 1.3 via rustls) into `<data>/agent/tls/`, key and token files at 0600. It prints the certificate's SHA-256 fingerprint at startup; remote clients **pin the fingerprint** — the fingerprint is the server identity. There is no hostname validation theater: a client may connect by IP or name, the pin is what proves the server. Rotating the identity means deleting the `tls/` directory and re-pinning; `--insecure-skip-verify` exists on the client seam as an explicit, documented escape hatch that still encrypts but proves nothing — never a default.

The pinning verifier compares SHA-256 digests in constant time; the token compare digests both sides first so length and content look identical to the compare.

### The token is the credential, enforced before any daemon contact

The agent's token is 32 random bytes, base64url, one line in `<data>/agent/token` (0600). The remote client's first frame must be `daemon.hello` with `auth` set; the agent validates it **before opening the local connection**, so an unauthenticated peer never causes a daemon session. Verdicts are typed errors carrying the client's own request id (Null id for unreadable first frames, mirroring the daemon's malformed-frame rule):

| Case | Reply |
|---|---|
| `auth` missing or empty | `AUTH_REQUIRED` |
| `auth` wrong | `AUTH_REJECTED`, connection closed |
| first frame not `daemon.hello` | `PROTOCOL_VERSION_UNSUPPORTED` (mirrors the daemon's first-exchange rule) |
| hello ok, daemon gone | `DAEMON_UNREACHABLE` |

An authenticated hello is forwarded **unchanged** — `auth` included — and the daemon ignores it on its local leg, exactly as the protocol spec has said since v0. One credential file, one rotation story: replace the file, restart the agent.

### Loopback by default

The agent binds `127.0.0.1:7443` unless told otherwise. Exposing a daemon to the network is an operator decision made once, visibly (`--listen 0.0.0.0:7443` behind whatever firewall the host owns), not a default that happens to someone.

## Consequences

- The phase proof is now mechanical: run the agent on the "headless" machine, point a remote-mode client (bridge/panel host, later CLI) at host:port with the token and the pinned fingerprint.
- Per-operation ACLs, audit logging, and a pairing flow stay deferred (the review's §14 list). The token gates the whole session, which is the honest scope for a single-operator product.
- Client transports that speak this (dev-bridge remote mode, Tauri host, CLI) reuse `zamin-agent`'s client seam; Node's TLS stack can pin the same fingerprint.
- The agent is one more binary to package; the installer and portable layout grow it alongside `zamind` and `zamin`.

## Alternatives considered

- **TCP+TLS listener inside `zamind`** — rejected: the daemon becomes network-facing, its single-instance and per-user model entangles with the network, and every protocol change now has to consider a remote attack surface. The agent keeps one shallow, auditable network process.
- **TLS client certificates instead of a token** — rejected: mTLS without a CA means distributing client certs and managing revocation; for a single operator the token + server pin gives the same practical security with none of the ceremony. Client certs remain an additive future option (the server config already takes `with_no_client_auth()` as the one seam).
- **SSH tunnel as the remote story** — rejected as the product answer: it is a fine *documented* alternative for tinkerers, but it does not satisfy "managed from a desktop Panel" on Windows, and it hides the protocol behind tooling the product does not control.
- **WebSocket transport for the browser directly** — rejected for now: a browser cannot pin a raw TCP fingerprint without a CA; the bridge/host remote modes keep TLS on the Rust/Node side where pinning is honest.
