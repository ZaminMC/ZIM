// The properties AST (§33): a line-preserving parse of Java-properties
// files. The editor never re-serializes the file from a rebuilt model —
// it keeps every raw line and rewrites only the pairs whose key or value
// actually changed, so comments, blank lines, ordering, CRLF endings,
// and keys the compose view does not know survive a save byte for byte
// (serialize re-parses each pair's own raw line and re-renders it only
// when it no longer decodes to the pair's current key/value — no
// mutation path can forget to mark itself dirty). Compose is a view
// over this AST, never a second representation that can drift.

export type PropertyLineKind = "blank" | "comment" | "pair";

export interface PropertyLine {
  kind: PropertyLineKind;
  /** The untouched raw text — for every kind, pairs included. A pair
   *  whose raw still decodes to its own key/value is emitted verbatim;
   *  the raw is the byte-for-byte round-trip. */
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

const ESCAPES: Record<string, string> = {
  "\\": "\\\\",
  "=": "\\=",
  ":": "\\:",
  " ": "\\ ",
  "#": "\\#",
  "!": "\\!",
  "\n": "\\n",
  "\r": "\\r",
  "\t": "\\t",
  "\f": "\\f",
};

/** Keys must escape everything the parser would treat as structure:
 *  separators (`=` `:` and whitespace — a space in a key would otherwise
 *  become the separator on re-parse), comment starters (`#` `!` — a key
 *  starting with one would become a comment), and the usual control
 *  characters. */
function encodeKey(text: string): string {
  let out = "";
  for (const ch of text) {
    const esc = ESCAPES[ch];
    if (esc !== undefined) {
      out += esc;
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

/** Values escape separators and control characters. A value's LEADING
 *  run of spaces/tabs is the separator's own whitespace to the parser —
 *  escape it or the round-trip eats it; INNER and trailing spaces are
 *  literal content in a value (unlike a key, where any space is
 *  structural) and stay unescaped. */
function encodeValue(text: string): string {
  let out = "";
  let leading = true;
  for (const ch of text) {
    if (leading && (ch === " " || ch === "\t")) {
      out += ESCAPES[ch] ?? ch;
      continue;
    }
    leading = false;
    if (ch === " " || ch === "\t") {
      out += ch; // inner/trailing: literal value content
      continue;
    }
    const esc = ESCAPES[ch];
    if (esc !== undefined) {
      out += esc;
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

/** Decode one pair line's body (no line ending) into key + value. A
 *  pair is `key sep value` where sep is the first unescaped `=`, `:`,
 *  or whitespace run; keys keep their case. A line that parses as
 *  nothing else is an honest pair with an empty value (properties
 *  semantics: a bare key means ""). */
function decodePairBody(body: string): { key: string; value: string } {
  let sep = -1;
  for (let i = 0; i < body.length; i += 1) {
    const ch = body[i];
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
    return { key: decodeValue(body.trim()), value: "" };
  }
  const keyPart = body.slice(0, sep).trim();
  let rest = body.slice(sep + 1);
  // One optional run of spaces after the separator is the format's own.
  const afterSep = rest.match(/^[ \t]*/)?.[0].length ?? 0;
  rest = rest.slice(afterSep);
  return { key: decodeValue(keyPart), value: decodeValue(rest) };
}

/** One file → lines. A CRLF file carries its `\r` at the end of every
 *  raw line — that is the line's ENDING, not part of its value: the CR
 *  is parsed off the body but kept on the raw, so an untouched file
 *  round-trips byte for byte and an edited line re-renders with the
 *  file's own ending. */
export function parseProperties(text: string): PropertyLine[] {
  const lines = text.split("\n");
  // A trailing newline is a real final line break, not an extra line.
  const trailingBreak = lines.length > 1 && lines.at(-1) === "";
  if (trailingBreak) lines.pop();
  const parsed: PropertyLine[] = lines.map((raw) => {
    const body = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
    const trimmed = body.trimStart();
    if (trimmed === "") return { kind: "blank", raw };
    if (trimmed.startsWith("#") || trimmed.startsWith("!")) return { kind: "comment", raw };
    const { key, value } = decodePairBody(body);
    return { kind: "pair", raw, key, value };
  });
  if (trailingBreak) parsed.push({ kind: "blank", raw: "" });
  return parsed;
}

/** Lines → file text. Blank and comment lines survive as their raw
 *  bytes; a pair survives as its own raw line while that raw still
 *  decodes to the pair's current key/value, and re-renders (key +
 *  `=` + encoded value, keeping the line's original CR ending) the
 *  moment it no longer does. */
export function serializeProperties(lines: PropertyLine[]): string {
  return lines
    .map((line) => {
      if (line.kind !== "pair") return line.raw;
      const key = line.key ?? "";
      const value = line.value ?? "";
      if (line.raw !== "") {
        const body = line.raw.endsWith("\r") ? line.raw.slice(0, -1) : line.raw;
        const asWritten = decodePairBody(body);
        if (asWritten.key === key && asWritten.value === value) return line.raw;
      }
      const cr = line.raw.endsWith("\r") ? "\r" : "";
      return `${encodeKey(key)}=${encodeValue(value)}${cr}`;
    })
    .join("\n");
}

/** An immutable edit: the pair named `key` gets `value`; everything
 *  else — including its position — stays exactly as it was.
 *
 *  LAST match wins, because that is Java's own load semantics: when a
 *  key is duplicated, `Properties.load` keeps the final assignment and
 *  the file's earlier copies are shadowed. Compose renders the winner
 *  (composeRows), so the edit lands on the line the server actually
 *  reads — a first-match edit would write a value the file silently
 *  ignores and the row would appear to "not stick". Shadowed copies
 *  are left byte-stable; Source shows and edits them. Setting a pair
 *  to the value it already carries is a no-op. */
export function setPropertyValue(
  lines: PropertyLine[],
  key: string,
  value: string,
): PropertyLine[] {
  let winner = -1;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (line?.kind === "pair" && line.key === key) winner = i;
  }
  const current = winner === -1 ? undefined : lines[winner];
  if (current === undefined || current.value === value) return lines;
  const next = lines.slice();
  next[winner] = { ...current, value };
  return next;
}

/** The pairs the Compose view renders, under the file's own load
 *  semantics (last-wins — the same law setPropertyValue edits by, so a
 *  row always shows and writes the line the server reads):
 *  - a duplicated key appears ONCE, as its final line; the earlier
 *    copies are counted in `shadowed` for the honest note;
 *  - a pair with an EMPTY key is not a renderable row (a blank label
 *    would lie about what it edits); it is counted in `uneditable`
 *    alongside comments and blanks, which Compose never touches.
 *  `rows` keys are unique by construction — safe React keys. */
export function composeRows(lines: PropertyLine[]): {
  rows: PropertyLine[];
  shadowed: number;
  uneditable: number;
} {
  const byKey = new Map<string, PropertyLine>();
  let shadowed = 0;
  let uneditable = 0;
  for (const line of lines) {
    if (line.kind !== "pair") {
      uneditable += 1;
      continue;
    }
    const key = line.key ?? "";
    if (key === "") {
      uneditable += 1;
      continue;
    }
    if (byKey.has(key)) shadowed += 1;
    // Last-wins: a later duplicate replaces the row in place (Map keeps
    // first insertion order — the row stays where the key first appeared
    // in the file) while its earlier copy joins the shadowed count.
    byKey.set(key, line);
  }
  return { rows: [...byKey.values()], shadowed, uneditable };
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
