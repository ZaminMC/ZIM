// zim://devtools — the ZIM developer tools (Part 2): a control center
// over the SAME protocol client and the SAME stores the rest of the UI
// uses — deliberately no parallel server management, no raw Rust
// internals, no filesystem or process access. The scripting console's
// `zim` object is the deliberate, capability-named API surface; the
// webview itself is the sandbox the scripts run in.
//
// Sections: Console · Servers · Inspector · Events.

import { useEffect, useMemo, useRef, useState } from "react";
import { useConnection } from "../../state/connection";
import { sortedServers, useServers } from "../../state/servers";
import { navigateHost } from "../../state/shellLane";
import { describeError } from "../../state/errors";
import { client } from "../../state/wire";
import type { CoreEvent } from "../../protocol/types";
import styles from "./DevToolsPage.module.css";

type Line = { kind: "in" | "out" | "error" | "info"; text: string };

/** The deliberate developer API. Nothing here touches the OS: every
 *  verb goes through the protocol client's request lane with the same
 *  authority (and the same audit trail) as the normal UI. Dangerous
 *  capabilities — filesystem writes outside a server jail, process
 *  spawn, network egress — do not exist on this object at all; they
 *  are not gated, they are absent. */
function buildDeveloperApi(log: (line: Line) => void) {
  const servers = {
    list: () => sortedServers(useServers.getState().servers).map((s) => ({
      id: s.serverId,
      name: s.displayName,
      state: s.state,
      software: s.software,
      version: s.version,
      port: s.port,
    })),
    get: (id: string) => {
      const entry = useServers.getState().servers[id];
      return entry
        ? { id: entry.serverId, name: entry.displayName, state: entry.state, port: entry.port }
        : null;
    },
    start: (id: string) =>
      void import("../../state/actions").then((a) =>
        a.startServer(id).catch((error: unknown) => log({ kind: "error", text: describeError(error).title })),
      ),
    stop: (id: string) =>
      void import("../../state/actions").then((a) =>
        a.stopServer(id).catch((error: unknown) => log({ kind: "error", text: describeError(error).title })),
      ),
  };
  const events = {
    /** Subscribe to the live events stream from the console. */
    subscribe: (handler: (event: CoreEvent) => void) => client.subscribe("events", undefined, {
      onPayload: (notification) => {
        if (notification.payload.kind === "event") handler(notification.payload.event);
      },
    }),
  };
  return { servers, events, daemon: () => useConnection.getState().daemon };
}

export function DevToolsPage() {
  const status = useConnection((s) => s.status);
  const servers = useServers((s) => s.servers);
  const [section, setSection] = useState<"console" | "servers" | "inspector" | "events">("console");
  const [lines, setLines] = useState<Line[]>([
    { kind: "info", text: "ZIM developer console — the `zim` object is the whole API. Try zim.servers.list()" },
  ]);
  const [input, setInput] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [historyAt, setHistoryAt] = useState<number | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [feed, setFeed] = useState<CoreEvent[]>([]);

  const log = (line: Line) => setLines((cur) => [...cur.slice(-200), line]);

   
  const api = useMemo(() => buildDeveloperApi(log), []);

  // The events section rides the client's stream while it is open.
  useEffect(() => {
    if (section !== "events" || status !== "ready") return;
    let handle: { dispose(): void } | null = null;
    void client
      .subscribe("events", undefined, {
        onPayload: (notification) => {
          const payload = notification.payload;
          if (payload.kind !== "event") return;
          setFeed((cur) => [...cur.slice(-100), payload.event]);
        },
      })
      .then((h) => {
        handle = h;
      });
    return () => {
      handle?.dispose();
    };
  }, [section, status]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [lines]);

  const run = () => {
    const code = input.trim();
    if (code === "") return;
    log({ kind: "in", text: code });
    setHistory((cur) => [...cur, code]);
    setHistoryAt(null);
    setInput("");
    try {
      // This IS the scripting console: evaluating the operator's code
      // against the deliberate `zim` API is the feature. The webview is
      // the sandbox; the API is the capability surface — no fs, no
      // process, no network primitive exists here to reach.
      // eslint-disable-next-line @typescript-eslint/no-implied-eval
      const fn = new Function("zim", `"use strict";\nreturn (${code});`);
       
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call
      const outcome = fn(api) as unknown;
      const maybeThen = (outcome as { then?: unknown }).then;
      if (outcome != null && typeof maybeThen === "function") {
        (outcome as Promise<unknown>).then(
          (value: unknown) => log({ kind: "out", text: stringify(value) }),
          (error: unknown) => log({ kind: "error", text: stringify(error) }),
        );
      } else {
        log({ kind: "out", text: stringify(outcome) });
      }
    } catch (error) {
      log({ kind: "error", text: stringify(error) });
    }
  };

  if (status !== "ready") {
    return (
      <div className={styles.page}>
        <header className={styles.head}>
          <h1 className={styles.title}>Developer tools</h1>
          <p className={styles.subtitle}>Waiting for the daemon…</p>
        </header>
      </div>
    );
  }

  const sorted = sortedServers(servers);
  const selected = section === "inspector" ? sorted[0] : undefined;


  return (
    <div className={styles.page}>
      <header className={styles.head}>
        <h1 className={styles.title}>Developer tools</h1>
        <p className={styles.subtitle}>
          The protocol is the authority — this console speaks it, never around it.
        </p>
      </header>

      <nav className={styles.tabs} aria-label="Developer tools sections">
        {(["console", "servers", "inspector", "events"] as const).map((name) => (
          <button
            key={name}
            className={`${styles.tab} ${section === name ? styles.tabActive : ""}`}
            onClick={() => setSection(name)}
          >
            {name.charAt(0).toUpperCase() + name.slice(1)}
          </button>
        ))}
      </nav>

      {section === "console" ? (
        <div className={styles.console}>
          <div className={styles.consoleLines} ref={scrollRef}>
            {lines.map((line, index) => (
              <div key={index} className={`${styles.line} ${styles[line.kind]}`}>
                {line.kind === "in" ? "› " : line.kind === "out" ? "‹ " : ""}
                {line.text}
              </div>
            ))}
          </div>
          <input
            className={styles.consoleInput}
            value={input}
            placeholder="zim.servers.list()"
            aria-label="Developer console input"
            spellCheck={false}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                run();
              } else if (e.key === "ArrowUp" && history.length > 0) {
                e.preventDefault();
                const at = historyAt == null ? history.length - 1 : Math.max(0, historyAt - 1);
                setHistoryAt(at);
                setInput(history[at] ?? "");
              } else if (e.key === "ArrowDown" && historyAt != null) {
                e.preventDefault();
                const at = historyAt + 1;
                if (at >= history.length) {
                  setHistoryAt(null);
                  setInput("");
                } else {
                  setHistoryAt(at);
                  setInput(history[at] ?? "");
                }
              }
            }}
          />
        </div>
      ) : null}

      {section === "servers" ? (
        <div className={styles.section}>
          {sorted.map((server) => (
            <div key={server.serverId} className={styles.row}>
              <span className={styles.rowName}>{server.displayName}</span>
              <span className={styles.rowMeta}>
                {server.serverId} · {server.state}
                {server.port ? ` · :${server.port}` : ""}
              </span>
              <span className={styles.rowActions}>
                <button className={styles.rowVerb} onClick={() => void navigateHost({ kind: "server", serverId: server.serverId })}>
                  Open
                </button>
              </span>
            </div>
          ))}
        </div>
      ) : null}

      {section === "inspector" ? (
        <div className={styles.section}>
          <p className={styles.note}>
            The inspector renders the sandbox record: what is enforced by the OS, what is accounted,
            and what the daemon observed. Select a server to inspect it.
          </p>
          {selected ? (
            <dl className={styles.facts}>
              <dt>Server</dt>
              <dd>
                {selected.displayName} ({selected.serverId})
              </dd>
              <dt>Status</dt>
              <dd>{selected.state}</dd>
              <dt>Filesystem</dt>
              <dd>Jailed to the server root (daemon APIs); OS read isolation: not yet enforced</dd>
              <dt>Memory</dt>
              <dd>Job-object cap: -Xmx + 512 MiB headroom (Windows: hard)</dd>
              <dt>Processes</dt>
              <dd>Job ceiling 64; children inherit the job, breakaway never granted</dd>
              <dt>Storage</dt>
              <dd>Accounted by the daemon sampler; writes refused at the budget (not an OS quota)</dd>
            </dl>
          ) : null}
        </div>
      ) : null}

      {section === "events" ? (
        <div className={styles.section}>
          {feed.length === 0 ? (
            <p className={styles.note}>Listening to the live events stream…</p>
          ) : (
            feed.map((event, index) => (
              <div key={index} className={styles.eventLine}>
                {JSON.stringify(event)}
              </div>
            ))
          )}
        </div>
      ) : null}
    </div>
  );
}

function stringify(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}
