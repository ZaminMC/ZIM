# ZIM host (Tauri 2)

The desktop shell per ADR-0003: **a bridge, nothing more**. The webview's
protocol client owns handshake, correlation, subscriptions, reconnect, and
cursors; this host owns the daemon connection's lifetime, frame forwarding,
and the ~50 ms coalescing of daemon → webview frames.

All bridge logic (framing, batching, down signals) lives in
[`crates/zamin-bridge`](../../../crates/zamin-bridge) — a workspace crate
tested like any other. This crate only wires it to Tauri's IPC channels:
three commands (`daemon_connect`, `daemon_send`, `daemon_close`) and two
channels (`frames`, `down`).

## Building

This crate is excluded from the Cargo workspace on purpose: Tauri pulls
machine-level system libraries that are not workspace dependencies.

Prerequisites:

- Linux: `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `libssl-dev`,
  `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`
  (see https://tauri.app/start/prerequisites/)
- Windows: WebView2 (preinstalled on Windows 11); Rust with the MSVC target

```sh
npm install            # in apps/panel (the UI)
npm run build          # produce ../dist for a bundled build
cargo build            # here, in src-tauri
cargo run              # dev: expects the vite dev server (npm run dev)
```

The window loads the built UI from `../dist` (or the dev server while
`tauri dev` runs); `daemon_connect` without arguments targets the per-user
endpoint, or pass `--endpoint` through the panel's connection settings once
they exist.

## CI

The Linux/Windows CI lanes build the workspace, not this crate. Add a
desktop lane (Ubuntu with the prerequisites above, Windows with MSVC) when
the panel enters packaging (Phase 7).
