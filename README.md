# ZaminPanel

A polished desktop and CLI workspace for running, managing, and developing Minecraft servers.

## Workspace layout

| Crate | Role |
|---|---|
| `crates/zamin-protocol` | Zamin Protocol v0: envelopes, typed params/results, error codes, conformance fixtures. Pure data — no I/O. |
| `crates/zamin-ipc` | Framed local transports: Windows named pipe, Unix domain socket. Client and server sides. |
| `crates/zamin-core` | The engine that owns servers: registry, config, filesystem safety, Java discovery, lifecycle, logs. |
| `crates/zamind` | The resident daemon (ADR-0001): supervision actors, event hub, protocol sessions, adoption. |
| `crates/zamin-cli` | `zamin` — the command line client (second protocol client, Phase 2). |
| `crates/testing/fake-mc-server` | Deterministic Paper mimic used by the whole test matrix — no Java needed. |

## The zamin CLI

The CLI talks to `zamind` over the local transport and never touches
`zamin-core` (ADR-0002: clients know the protocol, not internals).

```
zamin list                          # every server + state
zamin status <id>                   # one server's details
zamin daemon                        # daemon version, server counts
zamin register <id> <dir> [--name]  # register an existing server directory
zamin rename <id> --name "New"      # change the display name
zamin remove <id> [--yes]           # unregister (directory untouched)
zamin start|stop|restart|kill <id>  # lifecycle verbs (protocol §5)
zamin logs <id> [--lines N]         # tail of logs/latest.log (file-backed)
zamin logs <id> -f                  # follow: recent buffer, then live
zamin attach <id>                   # console: logs in, lines to stdin; /quit detaches
```

Global flags: `--endpoint <socket|pipe>` (default: the per-user endpoint),
`--json` (pretty JSON of the raw protocol result; errors print the typed
error object and exit 1), `--timeout <secs>`.

Exit codes: `0` success, `1` operation/protocol failure, `2` usage error.

## Development loop

```
cargo test --workspace    # unit + conformance + lifecycle + e2e (no Java needed)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

`cargo test --workspace` builds every binary the tests spawn (`zamind`,
`zamin`, `fake-mc-server`); run it rather than per-package tests so the
integration harnesses find their binaries.

## Documentation

- [Architecture review](docs/architecture/ARCHITECTURE-REVIEW.md) — decisions and implementation order (§23)
- [Protocol v0](docs/architecture/protocol-v0.md) — the client boundary
- [ADRs](docs/adr/) — accepted decisions 0001–0009
- [Style guide](docs/development/STYLE-GUIDE.md) · [Testing](docs/development/TESTING.md) · [Glossary](docs/development/GLOSSARY.md)
