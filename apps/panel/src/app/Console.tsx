// Server console: xterm.js terminal fed by the file-backed tail plus the
// live logs stream (ADR-0006), with typed lines to the server's stdin.
// Scrollback is capped (PERFORMANCE-BUDGETS); full history belongs to the
// log viewer, not terminal memory.

import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import type { ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import styles from "./Console.module.css";
import { sendStdin, subscribeLogs, rangeLogs } from "../state/actions";
import { formatLogLine, formatMarker, LineBuffer } from "./consoleText";
import type { LogLine } from "../protocol/types";
import { logWarn } from "../logger";

const SCROLLBACK = 5_000;
const HISTORY_LINES = 500;

function terminalTheme(): ITheme {
  const css = getComputedStyle(document.documentElement);
  const read = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  return {
    background: read("--term-bg", "#10141a"),
    foreground: read("--text", "#e7ecf3"),
    cursor: read("--accent", "#6ca7f2"),
    selectionBackground: "rgba(108, 167, 242, 0.30)",
  };
}

function fontFamily(): string {
  const css = getComputedStyle(document.documentElement);
  return css.getPropertyValue("--font-mono").trim() || "monospace";
}

export function Console({ serverId, running }: { serverId: string; running: boolean }) {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      scrollback: SCROLLBACK,
      fontFamily: fontFamily(),
      fontSize: 12.5,
      convertEol: true,
      cursorBlink: true,
      theme: terminalTheme(),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    try {
      fit.fit();
    } catch {
      // Headless or zero-size host: layout catches up on resize.
    }
    const observer = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        // Ignore fit failures; xterm keeps the last sane geometry.
      }
    });
    observer.observe(host);

    const life = { disposed: false };
    const isDisposed = () => life.disposed;

    const buffer = new LineBuffer();
    term.onData((data) => {
      const lines = buffer.push(data);
      for (const char of data) {
        if (char === "\r") term.write("\r\n");
        else if (char === "\u007f") term.write("\b \b");
        else if (char >= " ") term.write(char);
      }
      for (const line of lines) {
        void sendStdin(serverId, line).catch(() => {
          term.write("\x1b[31mstdin rejected: is the server running?\x1b[0m\r\n");
        });
      }
    });

    let disposeStream: (() => void) | null = null;

    void (async () => {
      // Subscribe FIRST: a fresh logs subscription replays the daemon's
      // recent in-memory ring as its opening batch (ADR-0006). The file
      // tail is the fallback for when that ring is empty (fresh daemon) —
      // reading the tail too would duplicate every replayed line.
      let registered = false;
      let openingBatches: LogLine[][] = [];
      let openingSeen = false;

      const sub = await subscribeLogs(serverId, {
        onPayload: (notification) => {
          if (isDisposed()) return;
          if (notification.payload.kind === "logs") {
            if (!openingSeen) {
              openingBatches.push(notification.payload.batch);
              return;
            }
            for (const line of notification.payload.batch) {
              term.writeln(formatLogLine(line));
            }
          } else if (notification.payload.kind === "missed") {
            term.writeln(formatMarker(`${notification.payload.missed} line(s) missed`));
          }
        },
        onRegistered: () => {
          if (life.disposed || !registered) return;
          term.writeln(formatMarker("reconnected to the daemon"));
        },
      });
      disposeStream = () => sub.dispose();
      registered = true;
      if (isDisposed()) {
        // StrictMode double-mount: cleanup ran while the subscribe was in
        // flight, so it could not dispose. Unregister immediately.
        sub.dispose();
        return;
      }

      // The opening replay batch is queued before any live line (hub
      // guarantees order under the subscribe lock).
      if (openingBatches.length > 0 && openingBatches.some((batch) => batch.length > 0)) {
        openingSeen = true;
        for (const batch of openingBatches) {
          for (const line of batch) term.writeln(formatLogLine(line));
        }
        openingBatches = [];
        term.writeln(formatMarker("recent buffer from the daemon"));
        return;
      }
      openingSeen = true;
      openingBatches = [];

      // Empty ring: fall back to the file-backed tail (ADR-0006).
      try {
        const tail = await rangeLogs({ serverId, maxLines: HISTORY_LINES });
        if (isDisposed()) return;
        if (tail.olderAvailable) {
          term.writeln(formatMarker("older lines live in the log file"));
        }
        for (const line of tail.lines) term.writeln(formatLogLine(line));
        term.writeln(formatMarker(`tail of ${tail.file}`));
      } catch {
        if (!isDisposed()) term.writeln(formatMarker("no log file yet"));
      }
    })().catch((error: unknown) => {
      logWarn("console", "console stream failed", error);
    });

    return () => {
      life.disposed = true;
      disposeStream?.();
      observer.disconnect();
      term.dispose();
    };
  }, [serverId]);

  return (
    <section className={styles.console} aria-label="Server console">
      <div className={styles.consoleBar}>
        <span>console</span>
        <span>{running ? "stdin connected" : "server is not running — stdin is rejected"}</span>
      </div>
      <div ref={hostRef} style={{ height: "340px" }} />
    </section>
  );
}
