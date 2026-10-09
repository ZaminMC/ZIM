// The popup overlay's boot — one transparent child webview that hosts
// the shell's application-owned menus and forms (the tab menu, the
// three-dot app menu, the group-name form). It is the DOM stand-in for
// Chromium's popup widgets: anchor-positioned by the host, focused on
// arrival, Escape/gutter-click to dismiss, verbs through the ONE command
// dispatch.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { PopupApp } from "./PopupApp";
import "../tokens.css";
import "./popup.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing from popup.html");
createRoot(root).render(
  <StrictMode>
    <PopupApp />
  </StrictMode>,
);
