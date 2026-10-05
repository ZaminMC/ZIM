# Contributing to ZaminPanel

ZaminPanel is the project home of three products: **ZaminPanel** (desktop GUI), **ZaminCLI** (`zamin`), and **ZaminCore** (the engine, running inside `zamind`). One repository, one engineering voice.

Start with: [ADR index](docs/adr/) · [Protocol v0](docs/architecture/protocol-v0.md) · [Style guide](docs/development/STYLE-GUIDE.md) · [Glossary](docs/development/GLOSSARY.md) · [Testing](docs/development/TESTING.md) · [Performance budgets](docs/development/PERFORMANCE-BUDGETS.md)

## Setup

**Windows:** Rust (stable, MSRV pinned in the workspace), Node LTS + pnpm, Visual Studio Build Tools, WebView2 (shipped with Windows 11).

**Linux:** Rust, Node LTS + pnpm, `webkit2gtk-4.1` development packages (per-distro names in `scripts/setup-linux.sh`), libudev.

```bash
cargo build --workspace        # system side
cd apps/panel && pnpm install  # UI side
cargo test --workspace         # everything except #[ignore]d perf tests
```

CI runs the same commands on Windows and Linux. If it fails in CI but not locally, your machine is not the reference — investigate, don't waive.

## How the standards are enforced

| Standard | Mechanism |
|---|---|
| Formatting | `rustfmt`, Prettier — non-negotiable, no discussion in review |
| Lints | clippy `-D warnings` with curated config (`unwrap` denied in libs, unbounded channels banned, `std::process::Command` banned outside `platform/`) |
| Platform seam | CI guard script: `#[cfg(windows)]`/`#[cfg(unix)]` outside `platform/` and `zamin-ipc` fails the build |
| Dependencies | `cargo-deny` (licenses, advisories, duplicates) |
| Commits & PR titles | conventional-commit lint |
| Spelling | `typos` |
| UI import boundaries | ESLint import rules (webview → protocol client + bridge only) |
| Docs | broken-link and rustdoc warning checks |
| Protocol | conformance fixtures on both platforms |

## Commits

Conventional Commits, one logical unit per commit:

```
feat(core): add server lifecycle manager
fix(daemon): preserve valid config after failed reload
feat(panel): add server workspace tabs
fix(panel): avoid blocking terminal updates
docs(protocol): define cursor invalidation semantics
ci: run linux gui smoke tests on webkit2gtk 4.1
```

Scopes: `core`, `daemon`, `cli`, `panel`, `protocol`, `docs`, `ci`, `build`, `tests`.

- The body explains *why* when the why isn't obvious from the diff. No generated summary paragraphs, no theatrical descriptions.
- No commits mixing formatting, refactoring, and behavior changes; no "implement everything" commits; no unrelated changes hitching a ride.
- Every commit passes fmt, clippy, and tests — a hook is provided (`scripts/hooks/`).

## Branches

Boring and descriptive: `feature/server-workspace`, `fix/linux-process-exit`, `refactor/core-config`, `docs/protocol-cursors`, `ci/perf-nightly`. No joke names, no names that don't describe the work.

## Pull requests

Use the PR template. Titles describe the actual change (`Add cross-platform process supervisor`), not marketing (`Implement amazing production-ready system`). A PR reads like an engineer explaining the change to another engineer: what changed, why, what was tested and on which platform, Windows/Linux considerations, known limitations. Simple change, simple description.

Review expectations — every PR is reviewed for:

- **Terminology**: uses the glossary; a new noun for an existing concept is a rejection (normalize it, don't layer it).
- **Unnecessary abstraction**: renaming wrappers, single-implementation interfaces, generic Manager/Service/Helper types, layers that exist for the diagram.
- **Under-abstraction**: choices that force rewrites later (see ADR list; the protocol, platform seam, and safety invariants are the usual suspects).
- **Comment quality**: narrating comments, comment banners, essay comments, or notes about how code was produced — all removed.
- **Control flow**: unnatural structure, defensive code for imaginary failures, error swallowing.
- **Duplication**: logic that already exists in a sibling subsystem.
- **Dependencies**: unjustified additions; barely-used deps get removed.
- **Oversized implementations** that should have been two commits or two modules.
- **Style drift** from the surrounding code.
- **Safety invariants** (process identity, fs containment, bounded queues) implemented partially or "as later hardening" — rejected outright.

Compiling and passing tests is the entry ticket, not the bar. If the structure is wrong, the structure changes — surface polish alone doesn't merge.

## Documentation voice

- Explain what the system actually does. Skip filler: *robust, seamless, cutting-edge, next-generation, highly scalable, revolutionary* — banned unless the term carries specific technical meaning in that sentence.
- Same vocabulary as the glossary, same verbs (start/stop/restart/kill), same error-message rules as the style guide.
- Docs state limitations honestly (e.g., TPS requires a plugin — say so rather than implying it exists).

## Reporting issues

Include: platform and version, what you did, what you expected, what happened, daemon log tail (`daemon.status` output if available), and for crashes the crash card contents. Security-sensitive reports (process spawning, path containment, IPC access) go to the maintainers directly before any public disclosure; a SECURITY.md lands before the repository is opened publicly.
