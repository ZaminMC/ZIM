// The CONTENT document's boot (ADR-0033): one tab's workspace. The
// frame (strip/toolbar/omnibox/bookmarks) boots from frame.html —
// this document is what a tab webview loads, one destination at a time.
//
// The corrected Arix port's boot law (2026-10-12): the app's ground is
// the official white (index.html paints it inline before the CSS
// lands); the DASHBOARD's theme (the scoped glass) is the store's own
// truth — applyTheme below mints the dialect variables on <html> BEFORE
// the first render, so the server view's scope paints its saved look
// immediately, and the broadcast lane keeps every open dashboard live.
// The splash rides index.html's self-contained inline CSS (the retire
// classes included — the rule lives WITH the markup it commands); this
// module only drives the classes.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import "./tokens.css";
import "./theme/arix.css";
import { applyTheme, useTheme } from "./state/theme";

performance.mark("panel:boot-start");

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("#root is missing from index.html");

// The theme's React-side truth: re-apply the saved shape (the inline
// pass and this agree — belt and suspenders around a first paint that
// must never see a default) and open the live lanes so a change made in
// ANY webview reaches this one in the same beat.
applyTheme(useTheme.getState().theme);
useTheme.getState().attachBroadcast();

createRoot(rootElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// THE SPLASH LAW: the first commit has painted underneath — retire the
// splash to a fade, then remove it from the tree once the transition
// ends. Two rAFs guarantee a rendered frame exists before the fade
// begins (the browser coalesces the class flip with the commit
// otherwise, and the splash would cut instead of cross-fade).
requestAnimationFrame(() =>
  requestAnimationFrame(() => {
    const splash = document.getElementById("boot");
    if (!splash) return;
    const fadeTarget = splash.firstElementChild ?? splash;
    fadeTarget.classList.add("boot-done");
    window.setTimeout(() => splash.classList.add("boot-gone"), 360);
  }),
);

requestAnimationFrame(() => {
  performance.mark("panel:interactive");
  try {
    performance.measure("panel:cold-start", "panel:boot-start", "panel:interactive");
  } catch {
    // A missing mark can only mean a non-measuring environment.
  }
});
