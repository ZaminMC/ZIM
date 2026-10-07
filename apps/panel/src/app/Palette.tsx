// Command palette (Ctrl/Cmd+K): the keyboard surface for the §53 journey.
// Commands are context-aware: lifecycle verbs for the active server are
// offered only when the state allows them.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { killServer, restartServer, startServer, stopServer } from "../state/actions";
import { describeError } from "../state/errors";
import type { LifecycleVerb } from "../state/errors";
import { autostartStatus, setAutostart } from "../integration/autostart";
import type { AutostartStatus } from "../integration/autostart";
import { availableVerbs } from "./ServerView";
import { useServers } from "../state/servers";
import { tabDestination, tabKeyOf, useTabs } from "../state/tabs";
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
  integration?: {
    autostart: AutostartStatus;
    toggleAutostart: () => void;
  },
): Command[] {
  const commands: Command[] = [
    { id: "new-server", label: "Register a new server…", run: actions.newServer },
  ];
  if (integration?.autostart.available) {
    // Login autostart (Phase 7): offered only where the host can deliver
    // it — a plain browser reports unavailable instead.
    commands.push({
      id: "autostart",
      label: integration.autostart.enabled
        ? "Stop starting with the system"
        : "Start with the system",
      hint: "login autostart",
      run: integration.toggleAutostart,
    });
  }
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
  // The palette's context is the active tab when it rests on a server.
  const activeTab = useTabs((s) => {
    const tab = s.tabs.find((t) => tabKeyOf(t) === s.activeKey) ?? s.tabs[0];
    const dest = tab ? tabDestination(tab) : undefined;
    return dest?.kind === "server" ? dest.serverId : null;
  });
  const setPending = useUi((s) => s.setPending);
  const activeServer = useServers((s) => (activeTab ? s.servers[activeTab] : undefined));

  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const [autostart, setAutostartState] = useState<AutostartStatus>({ available: false });
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    let live = true;
    void autostartStatus().then((status) => {
      if (live) setAutostartState(status);
    });
    return () => {
      live = false;
    };
  }, []);

  const toggleAutostart = useCallback(() => {
    const current = autostart;
    if (!current.available) return;
    close(false);
    // Optimistic flip, reconciled by the host's answer; a refusal puts the
    // honest state back. An unavailable host never gets here — the command
    // is hidden in that case.
    setAutostartState({ available: true, enabled: !current.enabled });
    void setAutostart(!current.enabled)
      .then((done) => {
        if (done) return;
        setAutostartState(current);
      })
      .catch(() => setAutostartState(current));
  }, [autostart, close]);

  const commands = useMemo(
    () =>
      buildCommands(
        activeTab,
        activeServer?.state ?? null,
        {
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
        },
        { autostart, toggleAutostart },
      ),
    [activeTab, activeServer?.state, autostart, close, openNewServer, setPending, toggleAutostart],
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
          setSelected((index) => Math.max(index - 1, filtered.length - 1));
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
