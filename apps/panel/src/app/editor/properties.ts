// The properties AST (§33): a line-preserving parse of Java-properties
// files. The editor never re-serializes the file from a rebuilt model —
// it keeps every raw line and rewrites only the pairs the operator
// touched, so comments, blank lines, ordering, and keys the compose view
// does not know survive a save byte for byte. Compose is a view over
// this AST, never a second representation that can drift.

export type PropertyLineKind = "blank" | "comment" | "pair";

export interface PropertyLine {
  kind: PropertyLineKind;
  /** The untouched raw text (for blank/comment) — the round-trip keeps it. */
  raw: string;
  /** For pairs: the key, decoded (backslash escapes resolved). */
  key?: string;
  /** For pairs: the value, decoded. */
  value?: string;
}

/** Decode the wire format of one logical line: `\:` `\=` `\ ` `\t` `\n`
 *  `\\` and `\uXXXX`. Properties files rarely use them, but the editor
 *  edits real files, not happy ones. */
function decodeValue(text: string): string {
  let out = "";
  for (let i = 0; i < text.length; i += 1) {
    const ch = text.charAt(i);
    if (ch !== "\\" || i + 1 >= text.length) {
      out += ch;
      continue;
    }
    const next = text.charAt(i + 1);
    if (next === "u" && i + 5 < text.length) {
      const hex = text.slice(i + 2, i + 6);
      if (/^[0-9a-fA-F]{4}$/.test(hex)) {
        out += String.fromCharCode(Number.parseInt(hex, 16));
        i += 5;
        continue;
      }
    }
    const simple: Record<string, string> = { n: "\n", t: "\t", r: "\r", f: "\f" };
    out += simple[next] ?? next;
    i += 1;
  }
  return out;
}

function encodeValue(text: string): string {
  let out = "";
  for (const ch of text) {
    if (ch === "\\" || ch === "=" || ch === ":" || ch === "\n" || ch === "\r" || ch === "\t" || ch === "\f") {
      const simple: Record<string, string> = {
        "\\": "\\\\",
        "=": "\\=",
        ":": "\\:",
        "\n": "\\n",
        "\r": "\\r",
        "\t": "\\t",
        "\f": "\\f",
      };
      out += simple[ch] ?? "";
      continue;
    }
    if (ch < " ") {
      out += `\\u${ch.charCodeAt(0).toString(16).padStart(4, "0")}`;
      continue;
    }
    out += ch;
  }
  return out;
}

/** One file → lines. A pair is `key sep value` where sep is the first
 *  unescaped `=`, `:`, or whitespace run; keys keep their case. A line
 *  that parses as nothing else is an honest `pair` with an empty value
 *  (properties semantics: a bare key means ""). */
export function parseProperties(text: string): PropertyLine[] {
  const lines = text.split("\n");
  // A trailing newline is a real final line break, not an extra line.
  const trailingBreak = lines.length > 1 && lines.at(-1) === "";
  if (trailingBreak) lines.pop();
  const parsed: PropertyLine[] = lines.map((raw) => {
    const trimmed = raw.trimStart();
    if (trimmed === "") return { kind: "blank", raw };
    if (trimmed.startsWith("#") || trimmed.startsWith("!")) return { kind: "comment", raw };
    // Split key from value at the first unescaped separator.
    let sep = -1;
    for (let i = 0; i < raw.length; i += 1) {
      const ch = raw[i];
      if (ch === "\\") {
        i += 1;
        continue;
      }
      if (ch === "=" || ch === ":") {
        sep = i;
        break;
      }
      if (ch === " " || ch === "\t") {
        sep = i;
        break;
      }
    }
    if (sep === -1) {
      return { kind: "pair", raw, key: decodeValue(raw.trim()), value: "" };
    }
    const keyPart = raw.slice(0, sep).trim();
    let rest = raw.slice(sep + 1);
    // One optional run of spaces after the separator is the format's own.
    const afterSep = rest.match(/^[ \t]*/)?.[0].length ?? 0;
    rest = rest.slice(afterSep);
    return { kind: "pair", raw, key: decodeValue(keyPart), value: decodeValue(rest) };
  });
  if (trailingBreak) parsed.push({ kind: "blank", raw: "" });
  return parsed;
}

/** Lines → file text. The raw bytes of blank and comment lines survive;
 *  a pair renders key + separator + encoded value on one line. */
export function serializeProperties(lines: PropertyLine[]): string {
  return lines
    .map((line) => {
      if (line.kind !== "pair") return line.raw;
      return `${encodeValue(line.key ?? "")}=${encodeValue(line.value ?? "")}`;
    })
    .join("\n");
}

/** An immutable edit: the pair named `key` (first match) gets `value`;
 *  everything else — including its position — stays exactly as it was. */
export function setPropertyValue(
  lines: PropertyLine[],
  key: string,
  value: string,
): PropertyLine[] {
  return lines.map((line) =>
    line.kind === "pair" && line.key === key ? { ...line, value } : line,
  );
}

/** `server-port` → "Server Port". Words split on `-`, `.`, `_`. */
export function humanizeKey(key: string): string {
  return key
    .split(/[-._]/)
    .filter((word) => word !== "")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

export type PropertyControl = "boolean" | "number" | "text";

/** The control a pair earns by its own bytes: exactly true/false is a
 *  toggle, a decimal integer is a number field, everything else is text.
 *  No hardcoded key list to drift out of date — the file speaks. */
export function controlFor(value: string): PropertyControl {
  if (value === "true" || value === "false") return "boolean";
  if (/^-?\d+$/.test(value.trim())) return "number";
  return "text";
}
