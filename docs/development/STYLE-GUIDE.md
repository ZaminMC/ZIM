# ZIM Style Guide

*One engineering voice across Rust, TypeScript, docs, and messages. Enforced mechanically where possible (see CONTRIBUTING); everywhere else by review.*

## Principles

1. **Obvious beats clever.** If an experienced developer wouldn't naturally write it this way for this problem, rewrite it.
2. **Simple because understood, not simple because corners were cut, and not complex because the diagram looked impressive.**
3. One vocabulary (see [GLOSSARY.md](GLOSSARY.md)). If two names exist for one concept, that is a bug in the codebase.
4. Differences in style between files come from the problem, never from a different author's habits.

## Rust

- Formatting: `rustfmt` defaults. No style churn in review.
- Lints: clippy with `-D warnings` in CI; `unwrap_used` and `expect_used` denied in `zamin-protocol`, `zamin-ipc`, `zamin-core` (tests and `main` functions excepted — use `expect` with context at process boundaries).
- Errors: `thiserror` enums; one error type per subsystem; convert to protocol error codes only at the IPC boundary. Never format errors into strings to pass them around.
- Async: tokio. No blocking calls on async threads — file I/O and process spawns that wait go through `spawn_blocking`. No `unbounded_channel` (banned by clippy config); every channel is bounded with a documented overflow policy.
- Structure: soft limits — functions ≈ 50 lines, modules ≈ 300 lines, nesting ≤ 3. Exceeding one is a signal to split by meaning, not by line count.
- Visibility: `pub(crate)` by default; `pub` only at crate boundaries (`zamin-protocol` types, core's facade to `zamind`).
- Construction: plain structs with builder-or-`new` where it reads well. No trait + single impl without a seam justification (see Abstraction discipline).
- No `unsafe` without a justification comment naming the invariant that makes it sound.

## TypeScript (Panel)

- `typescript-eslint` strict; named exports only; no classes — functions and modules.
- React function components; hooks per convention; no default exports, no `React.FC`.
- State: `zustand` stores per domain; the protocol client is a plain module with explicit reconnect/cursor logic — the only place reconnection is implemented.
- Styling: CSS Modules + tokens as CSS custom properties in one tokens file. No utility framework, no component library; the design system is `apps/panel/src/ui/`.
- The webview imports only: the protocol client module, the Tauri bridge module, UI code. Import boundaries are enforced by ESLint config, not convention.

## Comments

Explain why, never narrate what. Sparse, specific, natural.

```rust
// Good: explains a non-obvious constraint.
// Treat an exited JVM as crashed until the lifecycle handler confirms
// that shutdown was requested by the user.

// Bad: narrates the next line.
// Check if the server is running.
```

- No section banners (`// ===== SERVER MANAGEMENT =====`) unless the file genuinely needs the structure.
- Doc comments on public items in `zamin-protocol`; elsewhere only where the signature can't say it.
- No comments mentioning tools, models, prompts, or how the code was produced. Ever.
- A comment that outlives its reason gets deleted with the code that created it.

## Abstraction discipline

Rejected on sight: wrappers that rename a function; interfaces with one implementation and no seam; factories constructing one object; `Manager`/`Service`/`Helper`/`Util`/`Coordinator`/`Handler` type names; layers that exist for the diagram; defensive code for imaginary failures; patterns added as "best practice."

A seam is justified by exactly one of: (a) two real implementations exist, (b) it is the platform boundary (ADR-0008), (c) it is the protocol boundary (ADR-0002), (d) it makes a hard invariant enforceable (process identity, fs containment). If none apply, write the concrete thing.

## Error messages

- One sentence, sentence case, specific, naming the subject and cause: `Port 25565 is already in use by server 'production'.`
- Bare `Failed to <verb>` is banned — it states nothing the caller didn't know.
- User-facing message, log message, and protocol `message` may differ in detail but must use the same vocabulary (glossary) and the same verbs (start/stop/restart/kill).
- Every protocol error carries a code from the registry; ad-hoc codes are a review rejection.

## Logging

- `tracing` in the daemon. Levels:
  - `error` — an operation failed in a way the user must act on.
  - `warn` — degraded or unexpected but handled (retry, fallback, adoption anomaly).
  - `info` — lifecycle milestones only: daemon started, server started/stopped, job completed.
  - `debug` — protocol traffic summaries, decisions with alternatives considered.
  - `trace` — verbose detail, off by default.
- No narration (`entering X`, `got message`). If a log line wouldn't help diagnose a bug report, delete it.
- Message style: lowercase, terse, with identifiers — `server 'production' started (pid 4212, boot 3.2s)`.
- The webview ships no `console.log` in production builds; a dev-only logger module handles the rest.

## Dependencies

- A new dependency is a review topic, not a line item: name what it replaces and why `std`/`tokio`/`serde`/the existing UI stack can't do it.
- `cargo-deny` enforces licenses, advisories, and duplicate versions. Unused or barely-used dependencies get removed on sight.

## Repository layout hygiene

- File names match their primary type or role (`supervisor.rs`, `registry.rs`, `use-server-streams.ts`).
- One concept per module; a module named `utils` or `common` is rejected.
- Tests live beside the code (unit) or in `crates/*/tests/` (integration) — see [TESTING.md](TESTING.md).
