// The popup overlay's boot — one transparent child webview that hosts
// the shell's application-owned menus and forms (the tab menu, the
// three-dot app menu, the group-name form). It is the DOM stand-in for
// Chromium's popup widgets: anchor-positioned by the host, focused on
// arrival, Escape/gutter-click to dismiss, verbs through the ONE command
// dispatch.
//
// The Arix port's boot law (2026-10-12): the inline script painted the
// theme's tokens before this module ran; the store re-states them and
// opens the broadcast lane, so a studio change re-themes an OPEN menu
// in the same beat.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { PopupApp } from "./PopupApp";
import "../tokens.css";
import "./popup.css";
import { applyTheme, useTheme } from "../state/theme";

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing from popup.html");

applyTheme(useTheme.getState().theme);
useTheme.getState().attachBroadcast();

createRoot(root).render(
  <StrictMode>
    <PopupApp />
  </StrictMode>,
);
