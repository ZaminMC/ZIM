// The CONTENT document's boot (ADR-0033): one tab's workspace. The
// frame (strip/toolbar/omnibox/bookmarks) boots from frame.html —
// this document is what a tab webview loads, one destination at a time.
//
// The Arix port's boot law (2026-10-12): the inline script in index.html
// already painted the saved theme's ground before this module ran; here
// the store takes over — the same values re-stated through applyTheme,
// the cross-webview broadcast attached, and the splash retired with the
// app's first commit underneath it (the fade, never a pop).
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
