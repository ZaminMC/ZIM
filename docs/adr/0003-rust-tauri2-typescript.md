# ADR-0003 — Rust + Tauri 2 + TypeScript stack

**Status:** Accepted · **Date:** 2026-10-05

## Context

Hard requirements: performance and memory from day one, one coherent codebase, no runtime dependency for the manager itself, a UI that can reach a high design bar on Windows and Linux, and no unnecessary multi-language stack.

## Decision

| Part | Choice |
|---|---|
| Engine, daemon, CLI, protocol, test tools | **Rust** (one Cargo workspace) |
| Desktop shell | **Tauri 2** |
| Panel UI | **TypeScript + React**, function components only |
| Styling | CSS Modules + design tokens as CSS custom properties. No Tailwind, no component framework |
| UI state | `zustand`; the protocol client is a plain module, not a framework |
| Terminal | `xterm.js` (WebGL renderer, canvas fallback) |
| Editor | `CodeMirror 6` |

Boundary rules:

- The Tauri Rust host is a **bridge only**: connection management, reconnect, and forwarding. Zero business logic. If a feature needs logic in the host, it belongs in Core.
- High-frequency streams (logs, metrics) cross into the webview via Tauri's **Channel** API with host-side batching (~50 ms), never one emit per event.
- Multi-step flows (restore = stop → swap → start) are **Core jobs**, never client-side orchestration sequences.
- View state (open tabs, ordering, pins) is Panel-local storage, never daemon state.
- Test tooling (including `fake-mc-server`) is Rust in the workspace — no Python, no Node in the system side.

## Consequences

- The daemon and CLI ship with no runtime dependency; Java is needed only to launch servers.
- WebKitGTK variance on Linux is an owned risk: GNOME/KDE × Wayland/X11 smoke tests happen in the Panel phase, not later. Known toggles (`WEBKIT_DISABLE_DMABUF_RENDERER`, `WEBKIT_DISABLE_COMPOSITING_MODE`) surface through a diagnostics switch, not user folklore.
- React is chosen for its boring ecosystem, not for novelty; the design system is ours, built on tokens.

## Alternatives considered

- C#/.NET + Avalonia: viable fallback; weaker terminal/editor story, two runtimes.
- Kotlin/JVM: requires Java to manage Java — a bootstrap flaw.
- Electron: heaviest memory story; violates the performance requirement.
- Go: strong daemon/CLI, no GUI path that reaches the design bar.
