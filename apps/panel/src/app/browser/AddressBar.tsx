// The address bar (ADR-0015): the tool bar's honest interpreter. It rests
// showing the current destination's address (a server shows its join
// address when the port is known, its internal URL when it is not), and on
// focus it accepts all three dialects. Suggestions are dialect-appropriate
// and every one carries the text it commits, so Enter is never a guess.

import { useEffect, useMemo, useRef, useState } from "react";
import {
  destinationUrl,
  joinAddress,
  parseAddressInput,
  restingAddress,
  resolveJoin,
  searchServers,
  type AddressRequest,
  type Destination,
} from "../../state/destinations";
import type { ServerEntry } from "../../state/servers";
import { commitAddress } from "./commitAddress";
import styles from "./AddressBar.module.css";

interface Suggestion {
  label: string;
  detail: string;
  commitText: string;
  tone: "normal" | "miss";
}

function suggestionsFor(
  draft: string,
  entries: ServerEntry[],
  host: string,
): Suggestion[] {
  const text = draft.trim();
  if (text === "") return [];
  const request: AddressRequest = parseAddressInput(text);
  switch (request.kind) {
    case "internal": {
      const dest: Destination = request.destination;
      const known = dest.kind !== "missing";
      return [
        {
          label: destinationUrl(dest),
          detail: known
            ? dest.kind === "servers"
              ? "The fleet"
              : dest.kind === "new"
                ? "Discovery"
                : dest.kind === "settings"
                  ? "Panel settings"
                  : "Server"
            : "No such ZaminPanel page",
          commitText: text,
          tone: known ? "normal" : "miss",
        },
      ];
    }
    case "join": {
      const hit = resolveJoin(request, entries, host);
      if (!hit) {
        return [
          {
            label: `Nothing registered on :${request.port}`,
            detail: "A join address only opens a server this panel manages",
            commitText: text,
            tone: "miss",
          },
        ];
      }
      return [
        {
          label: hit.displayName,
          detail: joinAddress(hit, host),
          commitText: text,
          tone: "normal",
        },
      ];
    }
    case "query": {
      const hits = searchServers(request.text, entries).slice(0, 8);
      const rows: Suggestion[] = hits.map((e) => ({
        label: e.displayName,
        detail: e.port ? joinAddress(e, host) : e.serverId,
        // A suggestion commits an ADDRESS that resolves to the server —
        // the join address when the port is known, the internal URL when
        // it is not. A bare name would only be another query.
        commitText: e.port ? joinAddress(e, host) : `zaminpanel://server/${e.serverId}`,
        tone: "normal",
      }));
      rows.push({
        label: `Search servers for \u201c${request.text}\u201d`,
        detail: "Discovery",
        commitText: request.text,
        tone: "normal",
      });
      return rows;
    }
  }
}

export function AddressBar({
  destination,
  entries,
  host,
}: {
  destination: Destination;
  entries: ServerEntry[];
  host: string;
}) {
  const resting = restingAddress(destination, entries, host);
  const [draft, setDraft] = useState<string | null>(null);
  const [missPort, setMissPort] = useState<number | null>(null);
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const editing = draft !== null;
  const value = editing ? draft : resting;

  // The shell's Ctrl+L / F6 lands here.
  useEffect(() => {
    const onFocus = () => {
      inputRef.current?.focus();
      inputRef.current?.select();
    };
    window.addEventListener("zamin:focus-address", onFocus);
    return () => window.removeEventListener("zamin:focus-address", onFocus);
  }, []);

  // A destination change (tab switch, navigation) drops the draft and any
  // stale miss note.
  useEffect(() => {
    setDraft(null);
    setMissPort(null);
  }, [destination]);

  const suggestions = useMemo(
    () => (editing ? suggestionsFor(draft, entries, host) : []),
    [editing, draft, entries, host],
  );

  const commit = (text: string) => {
    const outcome = commitAddress(text, entries, host);
    if (outcome.kind === "join-miss") {
      setMissPort(outcome.port);
      inputRef.current?.select();
      return;
    }
    setMissPort(null);
    setDraft(null);
    inputRef.current?.blur();
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      setDraft(null);
      setMissPort(null);
      inputRef.current?.blur();
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setSelected((i) => Math.min(i + 1, suggestions.length - 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setSelected((i) => Math.max(i - 1, 0));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const picked = suggestions[selected];
      commit(picked ? picked.commitText : (draft ?? ""));
    }
  };

  return (
    <div className={styles.wrap}>
      <input
        ref={inputRef}
        className={`${styles.input} ${editing ? styles.editing : ""}`}
        value={value}
        placeholder="Search servers, a join address, or a zaminpanel:// page"
        aria-label="Address bar"
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => {
          setDraft(e.target.value);
          setSelected(0);
          setMissPort(null);
        }}
        onFocus={(e) => {
          setDraft(resting);
          requestAnimationFrame(() => e.target.select());
        }}
        onKeyDown={onKeyDown}
      />
      {missPort !== null ? (
        <div className={styles.missNote} role="status">
          Nothing registered on :{missPort}
        </div>
      ) : null}
      {editing && suggestions.length > 0 ? (
        <ul className={styles.suggestions} role="listbox" aria-label="Address suggestions">
          {suggestions.map((s, i) => (
            <li
              key={`${s.label}:${i}`}
              role="option"
              aria-selected={i === selected}
              className={`${styles.row} ${i === selected ? styles.rowActive : ""} ${
                s.tone === "miss" ? styles.rowMiss : ""
              }`}
              // Prevent blur before the click lands.
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => commit(s.commitText)}
              onMouseEnter={() => setSelected(i)}
            >
              <span className={styles.label}>{s.label}</span>
              <span className={styles.detail}>{s.detail}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
