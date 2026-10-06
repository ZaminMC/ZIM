// Pure console-text helpers: log lines → ANSI, keystrokes → lines. Kept
// free of xterm so tests cover them without a canvas.

import type { LogLine, LogLevel } from "../protocol/types";

const ANSI = {
  reset: "\x1b[0m",
  dim: "\x1b[2m",
  yellow: "\x1b[33m",
  red: "\x1b[31m",
  cyan: "\x1b[36m",
} as const;

function levelAnsi(level: LogLevel): string | null {
  switch (level) {
    case "warn":
      return ANSI.yellow;
    case "error":
      return ANSI.red;
    case "debug":
      return ANSI.cyan;
    default:
      return null;
  }
}

/** One log line as terminal output: dim thread prefix, level color. */
export function formatLogLine(line: LogLine): string {
  const color = levelAnsi(line.level);
  const prefix = line.thread ? `${ANSI.dim}[${line.thread}]${ANSI.reset} ` : "";
  return color ? `${prefix}${color}${line.line}${ANSI.reset}` : prefix + line.line;
}

/** Dim, unobtrusive seam markers (reconnect, missed count, older history). */
export function formatMarker(text: string): string {
  return `${ANSI.dim}— ${text} —${ANSI.reset}`;
}

/**
 * Keystroke collector for the console input line. xterm hands us single
 * characters: printable text accumulates, Enter (\r) completes the line,
 * DEL (\u007f) erases. Returns the completed lines in order.
 */
export class LineBuffer {
  private buffer = "";
  private readonly maxLineLength = 2_000;

  /** Feed one keystroke; returns completed lines. */
  push(input: string): string[] {
    const completed: string[] = [];
    for (const char of input) {
      if (char === "\r") {
        completed.push(this.buffer);
        this.buffer = "";
      } else if (char === "\u007f") {
        this.buffer = this.buffer.slice(0, -1);
      } else if (char >= " ") {
        // Printable range; keeps the buffer safe from paste bombs.
        if (this.buffer.length < this.maxLineLength) this.buffer += char;
      }
      // Other control characters are ignored.
    }
    return completed;
  }

  get pending(): string {
    return this.buffer;
  }
}
