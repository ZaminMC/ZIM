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
    // Three documents ship: the frame (the shell's view, ADR-0033), the
    // content app (one per tab webview), and the popup overlay
    // (application-owned menus and forms).
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        frame: resolve(__dirname, "frame.html"),
        popup: resolve(__dirname, "popup.html"),
      },
    },
  },
});
