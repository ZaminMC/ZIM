import "./boot/windowBoot";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import "./tokens.css";

// Cold-start measurement marks (PERFORMANCE-BUDGETS, Panel): module eval
// → first React commit. Read with
// `performance.getEntriesByName("panel:boot-start" | "panel:interactive")`
// in a browser or the Tauri webview inspector; the smoke run records the
// numbers in the budgets doc.
performance.mark("panel:boot-start");

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("#root is missing from index.html");

createRoot(rootElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// The first commit flushes after paint scheduling; requestAnimationFrame
// after render is the closest honest "interactive" point without adding
// dependencies.
requestAnimationFrame(() => {
  performance.mark("panel:interactive");
  try {
    performance.measure("panel:cold-start", "panel:boot-start", "panel:interactive");
  } catch {
    // A missing mark can only mean a non-measuring environment.
  }
});
