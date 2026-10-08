// The frame webview's boot — the shell's VIEW process (ADR-0033).
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { FrameApp } from "./FrameApp";
import "../tokens.css";
import "./frame.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing from frame.html");
createRoot(root).render(
  <StrictMode>
    <FrameApp />
  </StrictMode>,
);
