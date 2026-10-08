// The chrome webview's boot — the shell's VIEW process (ADR-0033).
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ChromeApp } from "./ChromeApp";
import "../tokens.css";
import "./chrome.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing from chrome.html");
createRoot(root).render(
  <StrictMode>
    <ChromeApp />
  </StrictMode>,
);
