import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "es2022",
    sourcemap: true,
    // Two documents ship: the chrome (the shell's view, ADR-0033) and
    // the content app (one per tab webview).
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        chrome: resolve(__dirname, "chrome.html"),
      },
    },
  },
});
