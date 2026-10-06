// Command palette (Ctrl/Cmd+K): the keyboard surface for the §53 journey.
// Commands are context-aware: lifecycle verbs for the active server are
// offered only when the state allows them.

import { useEffect, useMemo, useRef, useState } from "react";
import { killServer, restartServer, startServer, stopServer } from "../state/actions";
import { describeError } from "../state/errors";
import type { LifecycleVerb } from "../state/errors";
import { availableVerbs } from "./ServerView";
import { useServers } from "../state/servers";
import { useUi } from "../state/ui";
import styles from "./Palette.module.css";

export interface Command {
  id: string;
  label: string;
  hint?: string;
  disabled?: boolean;
  run: () => void;
}

export function buildCommands(
  activeServerId: string | null,
  activeState: string | null,
  actions: {
    newServer: () => void;
    lifecycle: (verb: LifecycleVerb) => void;
  },
): Command[] {
  const commands: Command[] = [
    { id: "new-server", label: "Register a new server…", run: actions.newServer },
  ];
  if (activeServerId && activeState) {
    const verbs = availableVerbs(activeState as never);
    const capitalize = (verb: string) => verb.charAt(0).toUpperCase() + verb.slice(1);
    for (const verb of ["start", "stop", "restart", "kill"] as const) {
      commands.push({
        id: `verb-${verb}`,
        label: `${capitalize(verb)} the active server`,
        hint: activeServerId,
        disabled: !verbs.includes(verb),
        run: () => actions.lifecycle(verb),
      });
    }
  }
  return commands;
}

export function Palette() {
  const close = useUi((s) => s.setPaletteOpen);
  const openNewServer = useUi((s) => s.setNewServerOpen);
  const activeTab = useUi((s) => s.activeTab);
  const setPending = useUi((s) => s.setPending);
  const activeServer = useServers((s) => (activeTab ? s.servers[activeTab] : undefined));

  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const commands = useMemo(
    () =>
      buildCommands(activeTab, activeServer?.state ?? null, {
        newServer: () => {
          close(false);
          openNewServer(true);
        },
        lifecycle: (verb) => {
          if (!activeTab) return;
          const serverId = activeTab;
          close(false);
          setPending(serverId, verb);
          const run =
            verb === "start"
              ? startServer
              : verb === "stop"
                ? stopServer
                : verb === "restart"
                  ? restartServer
                  : killServer;
          void run(serverId)
            .catch((error: unknown) => {
              const described = describeError(error);
              useUi.getState().setActionError(serverId, {
                code: described.code,
                message: described.title,
                remediation: described.remediation,
              });
            })
            .finally(() => setPending(serverId, null));
        },
      }),
    [activeTab, activeServer?.state, close, openNewServer, setPending],
  );

  const filtered = commands.filter((command) =>
    `${command.label} ${command.hint ?? ""}`.toLowerCase().includes(query.toLowerCase()),
  );
  const selectedIndex = Math.min(selected, Math.max(0, filtered.length - 1));

  const runSelected = () => {
    const command = filtered[selectedIndex];
    if (!command || command.disabled) return;
    command.run();
  };

  return (
    <div
      className={styles.palette}
      role="dialog"
      aria-label="Command palette"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          close(false);
        } else if (event.key === "ArrowDown") {
          event.preventDefault();
          setSelected((index) => Math.min(index + 1, filtered.length - 1));
        } else if (event.key === "ArrowUp") {
          event.preventDefault();
          setSelected((index) => Math.max(index - 1, 0));
        } else if (event.key === "Enter") {
          event.preventDefault();
          runSelected();
        }
      }}
    >
      <input
        ref={inputRef}
        className={styles.input}
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          setSelected(0);
        }}
        placeholder="Type a command…"
        aria-label="Search commands"
      />
      <ul className={styles.list}>
        {filtered.map((command, index) => (
          <li
            key={command.id}
            className={[
              styles.item,
              index === selectedIndex ? styles.itemSelected : "",
              command.disabled ? styles.itemDisabled : "",
            ]
              .filter(Boolean)
              .join(" ")}
            onMouseEnter={() => setSelected(index)}
            onMouseDown={(event) => {
              event.preventDefault();
              setSelected(index);
              runSelected();
            }}
          >
            <span>{command.label}</span>
            {command.hint ? <span className={styles.hint}>{command.hint}</span> : null}
          </li>
        ))}
        {filtered.length === 0 ? <li className={styles.empty}>No matching command.</li> : null}
      </ul>
    </div>
  );
}
