// The frame webview's boot — the shell's VIEW process (ADR-0033).
//
// The Arix port's boot law (2026-10-12): same as the content documents
// — the inline script painted the theme's ground, the store re-states
// it, the broadcast lane opens so a studio change anywhere re-themes
// the band live, and the splash fades over the band's first commit.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { FrameApp } from "./FrameApp";
import "../tokens.css";
import "./frame.css";
import "./frame-overrides.css";
import { applyTheme, useTheme } from "../state/theme";

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("#root is missing from frame.html");

applyTheme(useTheme.getState().theme);
useTheme.getState().attachBroadcast();

createRoot(rootElement).render(
  <StrictMode>
    <FrameApp />
  </StrictMode>,
);

requestAnimationFrame(() =>
  requestAnimationFrame(() => {
    const splash = document.getElementById("boot");
    if (!splash) return;
    const fadeTarget = splash.firstElementChild ?? splash;
    fadeTarget.classList.add("boot-done");
    window.setTimeout(() => splash.classList.add("boot-gone"), 360);
  }),
);
