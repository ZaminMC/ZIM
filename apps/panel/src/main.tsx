// The CONTENT document's boot (ADR-0033): one tab's workspace. The
// frame (strip/toolbar/omnibox/bookmarks) boots from frame.html —
// this document is what a tab webview loads, one destination at a time.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import "./tokens.css";

performance.mark("panel:boot-start");

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("#root is missing from index.html");

createRoot(rootElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

requestAnimationFrame(() => {
  performance.mark("panel:interactive");
  try {
    performance.measure("panel:cold-start", "panel:boot-start", "panel:interactive");
  } catch {
    // A missing mark can only mean a non-measuring environment.
  }
});
