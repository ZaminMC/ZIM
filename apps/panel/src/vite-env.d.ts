/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Dev bridge WebSocket URL for browser development (no Tauri host). */
  readonly VITE_BRIDGE_URL?: string;
}
