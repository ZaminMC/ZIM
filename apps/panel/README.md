# ZaminPanel — desktop UI

The Panel is a Tauri 2 app (ADR-0003): a thin Rust host that bridges the
webview ⇄ daemon transport, and a TypeScript UI that owns the protocol
client, state, and everything visible. The Rust host lives in `src-tauri/`
and is built together with the app when system libraries are available; the
UI develops and tests standalone in a plain browser.

## Layout

```
src/
  protocol/    # wire layer: types mirror of zamin-protocol, transport seam,
               # the client (handshake, correlation, subscriptions, reconnect)
  bridge/      # the only module that may import @tauri-apps
  state/       # zustand stores (connection, servers) + typed daemon actions
  ui/          # design-system primitives (tokens.css + CSS Modules)
  app/         # shell: sidebar, tabs, palette, views
dev-bridge.mjs # DEV ONLY: browser WebSocket ⇄ daemon's framed local transport
```

Import boundaries are enforced by ESLint (`no-restricted-imports`): the
webview imports only the protocol client, the bridge module, and UI code.

## Development (browser, no Tauri)

Three terminals, or one shell:

```
cargo build -p zamind                       # the daemon
./target/debug/zamind --endpoint /tmp/zamind.sock --data-dir ./data
node dev-bridge.mjs --endpoint /tmp/zamind.sock --port 8787
npm run dev                                 # vite on http://127.0.0.1:5173
```

The UI detects the Tauri host and falls back to the WebSocket bridge
(`VITE_BRIDGE_URL` overrides `ws://127.0.0.1:8787`). Register a server from
another terminal — `zamin register <id> <dir>` — and watch it appear live:
the events stream is the only reconcile channel (ADR-0006).

## Quality gates

```
npm test          # vitest: protocol client, stores, shell rendering
npm run typecheck # tsc --noEmit (strict, noUncheckedIndexedAccess)
npm run lint      # eslint (typescript-eslint strict + boundaries)
npm run build     # production build
```

`node dev-bridge.mjs`, `npm run dev`, and the browser smoke are the manual
vertical-slice check (see docs/development/TESTING.md).
