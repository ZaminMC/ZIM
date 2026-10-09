// File browser + editor (founder §32): the server root over the wire
// (spec §8). The listing pages directories-first; a text file opens in
// the editor and saves through the staged upload (chunked write → one
// atomic commit), so a crashed tab can never leave a half-written file
// at the target path. The slice's verbs live here too: search walks the
// whole root (bounded, the walk says when it truncated), copy refuses to
// overwrite (the typed refusal is the message), move is the one-rename
// path, upload lands through the same staging, and download streams the
// bytes back to the operator's machine.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  copyFilesEntry,
  deleteEntry,
  listFiles,
  mkdir,
  readWholeFile,
  renameEntry,
  searchFiles,
  writeWholeFile,
} from "../state/actions";
import type {
  FilesEntry,
  FilesListResult,
  FilesSearchResult,
} from "../protocol/types";
import {
  parseProperties,
  serializeProperties,
  setPropertyValue,
  humanizeKey,
  controlFor,
} from "./editor/properties";
import type { PropertyLine } from "./editor/properties";
import { reloadPlanFor } from "./editor/reload";
import { specializedEditorFor } from "./editor/registry";
import {
  readScoreboard,
  writeScoreboardTitle,
  writeScoreboardRows,
} from "./editor/scoreboard";
import { ScoreboardEditor } from "./editor/ScoreboardEditor";
import { restartServer } from "../state/actions";
import { useServers } from "../state/servers";
import { describeError } from "../state/errors";
import { Button } from "../ui/Button";
import { useDeferredWindow } from "../ui/deferred";
import styles from "./FilesView.module.css";

const EDIT_LIMIT_BYTES = 1024 * 1024; // the editor is for text files
// A download builds one Uint8Array in the tab before the browser takes
// over; the honest cap keeps a world folder from crashing the view —
// bigger trees go through backups, which stream.
const DOWNLOAD_LIMIT_BYTES = 100 * 1024 * 1024;
const SEARCH_DEBOUNCE_MS = 250;

// The deferred window resets on array identity; a fresh [] per render
// would reset it every time, so the empty fallback is a constant.
const NO_ENTRIES: FilesEntry[] = [];

function formatSize(bytes?: number): string {
  if (bytes === undefined) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GiB`;
}

function formatWhen(ms?: number): string {
  if (ms === undefined) return "";
  return new Date(ms).toLocaleString();
}

/** Root-relative POSIX path of the entry, relative to `dir`. */
function join(dir: string, name: string): string {
  if (dir === "" || dir === ".") return name;
  return `${dir}/${name}`;
}

function parentOf(dir: string): string {
  const cut = dir.lastIndexOf("/");
  return cut === -1 ? "" : dir.slice(0, cut);
}

/** Text sniffing: the editor edits text; binaries open read-only with a note. */
function looksLikeText(bytes: Uint8Array): boolean {
  const probe = bytes.subarray(0, 4096);
  for (const byte of probe) {
    if (byte === 0) return false;
  }
  return true;
}

export function FilesView({ serverId }: { serverId: string }) {
  const [dir, setDir] = useState("");
  const [listing, setListing] = useState<FilesListResult | null>(null);
  const [error, setError] = useState<{ message: string; code?: string } | null>(null);
  const [loading, setLoading] = useState(false);

  // Editor state: which file is open, its bytes as text, dirty flag.
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [saved, setSaved] = useState("");
  const [busy, setBusy] = useState(false);
  // §33's two modes: Compose is the friendly view over the properties
  // AST; Source is the raw file. Source is always one click away, for
  // every file, forever. §34 adds the specialized editor: when the
  // registry claims this file (e.g. a scoreboard configuration), its
  // mode rides the SAME compose pipeline — the AST — so every view is
  // a face over one model, never a second representation.
  const [composeMode, setComposeMode] = useState(false);
  const [specializedMode, setSpecializedMode] = useState(false);

  // Search (the whole-root walk) is a view over its own state: the box's
  // text, the debounced query actually sent, and the bounded answer.
  const [searchBox, setSearchBox] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResult, setSearchResult] = useState<FilesSearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const uploadInput = useRef<HTMLInputElement>(null);

  const refresh = useCallback(
    (path: string) => {
      setLoading(true);
      setError(null);
      void listFiles({ serverId, path, offset: 0, limit: 2000 })
        .then((result) => {
          setListing(result);
          setDir(path);
        })
        .catch((cause: unknown) => {
          const described = describeError(cause);
          setError({ message: described.title, code: described.code });
        })
        .finally(() => setLoading(false));
    },
    [serverId],
  );

  useEffect(() => {
    refresh("");
  }, [refresh]);

  // The search box debounces before the walk: typing one keystroke per
  // letter must not walk the whole root per letter.
  useEffect(() => {
    const timer = window.setTimeout(() => setSearchQuery(searchBox.trim()), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [searchBox]);

  useEffect(() => {
    if (searchQuery === "") {
      setSearchResult(null);
      setSearching(false);
      return;
    }
    setSearching(true);
    let alive = true;
    void searchFiles(serverId, searchQuery)
      .then((result) => {
        if (alive) setSearchResult(result);
      })
      .catch((cause: unknown) => {
        if (alive) {
          const described = describeError(cause);
          setError({ message: described.title, code: described.code });
          setSearchResult(null);
        }
      })
      .finally(() => {
        if (alive) setSearching(false);
      });
    return () => {
      alive = false;
    };
  }, [serverId, searchQuery]);

  const openFile = useCallback(
    (path: string, size?: number) => {
      if (size !== undefined && size > EDIT_LIMIT_BYTES) {
        setError({
          message: `${path} is ${formatSize(size)} — the editor covers text files up to 1 MiB.`,
        });
        return;
      }
      setBusy(true);
      setError(null);
      void readWholeFile(serverId, path)
        .then((bytes) => {
          if (!looksLikeText(bytes)) {
            setError({ message: `${path} looks binary — the editor edits text files.` });
            return;
          }
          const text = new TextDecoder().decode(bytes);
          setOpenPath(path);
          setDraft(text);
          setSaved(text);
        })
        .catch((cause: unknown) => {
          const described = describeError(cause);
          setError({ message: described.title, code: described.code });
        })
        .finally(() => setBusy(false));
    },
    [serverId],
  );

  const save = useCallback(() => {
    if (openPath === null) return;
    setBusy(true);
    setError(null);
    const bytes = new TextEncoder().encode(draft);
    void writeWholeFile(serverId, openPath, bytes)
      .then(() => {
        setSaved(draft);
        // Sizes may have changed; refresh whatever directory holds it.
        refresh(dir);
      })
      .catch((cause: unknown) => {
        const described = describeError(cause);
        setError({ message: described.title, code: described.code });
      })
      .finally(() => setBusy(false));
  }, [serverId, openPath, draft, dir, refresh]);

  const openDirEntry = useCallback(
    (entry: FilesEntry) => {
      const path = join(dir, entry.name);
      if (entry.symlinkOutside) {
        setError({
          message: `${entry.name} is a symlink outside the server root — ZIM will not follow it.`,
          code: "FS_PATH_ESCAPES_ROOT",
        });
        return;
      }
      if (entry.kind === "directory") {
        setOpenPath(null);
        refresh(path);
      } else {
        openFile(path, entry.sizeBytes);
      }
    },
    [dir, refresh, openFile],
  );

  const crumbs = useMemo(() => {
    const parts = dir === "" || dir === "." ? [] : dir.split("/");
    return parts.map((part, index) => ({
      name: part,
      path: parts.slice(0, index + 1).join("/"),
    }));
  }, [dir]);

  const act = useCallback(
    (run: () => Promise<unknown>, note: string) => {
      setBusy(true);
      setError(null);
      void run()
        .then(() => refresh(dir))
        .catch((cause: unknown) => {
          const described = describeError(cause);
          setError({ message: `${note}: described.title`, code: described.code });
        })
        .finally(() => setBusy(false));
    },
    [dir, refresh],
  );

  // The verbs of the slice. Copy asks for the destination (the typed
  // refusal names the honest mistake when it exists); move is the
  // one-rename path and shows it as such; download streams the file
  // back with the browser's own save affordance.
  const copyEntry = useCallback(
    (entry: FilesEntry) => {
      const from = join(dir, entry.name);
      const suggested = entry.name.includes(".")
        ? `${entry.name.slice(0, entry.name.lastIndexOf("."))} copy${entry.name.slice(entry.name.lastIndexOf("."))}`
        : `${entry.name} copy`;
      const to = window.prompt(`Copy ${entry.name} to (server-root path):`, join(dir, suggested));
      if (!to || to === from) return;
      act(
        () =>
          copyFilesEntry(serverId, from, to).then(() => {
            setSearchBox("");
          }),
        "copy failed",
      );
    },
    [dir, act, serverId],
  );

  const moveEntry = useCallback(
    (entry: FilesEntry) => {
      const from = join(dir, entry.name);
      const to = window.prompt(`Move ${from} to (server-root path):`, from);
      if (!to || to === from) return;
      act(() => renameEntry(serverId, from, to), "move failed");
    },
    [dir, act, serverId],
  );

  const downloadEntry = useCallback(
    (entry: FilesEntry) => {
      if (entry.kind !== "file") return;
      if (entry.sizeBytes !== undefined && entry.sizeBytes > DOWNLOAD_LIMIT_BYTES) {
        setError({
          message: `${entry.name} is ${formatSize(entry.sizeBytes)} — download covers files up to ${formatSize(DOWNLOAD_LIMIT_BYTES)}; use a backup for bigger trees.`,
        });
        return;
      }
      setBusy(true);
      setError(null);
      void readWholeFile(serverId, join(dir, entry.name))
        .then((bytes) => {
          const blob = new Blob([bytes], { type: "application/octet-stream" });
          const url = URL.createObjectURL(blob);
          const anchor = document.createElement("a");
          anchor.href = url;
          anchor.download = entry.name;
          anchor.click();
          window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
        })
        .catch((cause: unknown) => {
          const described = describeError(cause);
          setError({ message: described.title, code: described.code });
        })
        .finally(() => setBusy(false));
    },
    [dir, serverId],
  );

  const uploadFiles = useCallback(
    (files: FileList | File[]) => {
      const list = Array.from(files);
      if (list.length === 0) return;
      setBusy(true);
      setError(null);
      void Promise.all(
        list.map((file) =>
          file
            .arrayBuffer()
            .then((buffer) => writeWholeFile(serverId, join(dir, file.name), new Uint8Array(buffer))),
        ),
      )
        .then(() => refresh(dir))
        .catch((cause: unknown) => {
          const described = describeError(cause);
          setError({ message: `upload failed: described.title`, code: described.code });
        })
        .finally(() => setBusy(false));
    },
    [dir, refresh, serverId],
  );

  const dirty = draft !== saved;

  // §35: the plan comes from the path, not from guessing. A restart
  // needs the server to actually be running — otherwise the save alone
  // is the whole story (the next boot applies it anyway).
  const running = useServers((state) => state.servers[serverId]?.state === "running");
  const plan = useMemo(
    () => (openPath === null ? { restartable: false } : reloadPlanFor(openPath)),
    [openPath],
  );

  // The compose AST is rebuilt from the draft whenever mode or file
  // changes — one parser, one truth; edits go through setPropertyValue
  // so a compose keystroke can never reserialize untouched lines.
  const [composeFile, setComposeFile] = useState<{ path: string; lines: PropertyLine[] } | null>(null);
  useEffect(() => {
    if (!composeMode || openPath === null) {
      setComposeFile(null);
      return;
    }
    setComposeFile({ path: openPath, lines: parseProperties(draft) });
    // draft intentionally not a dependency: the AST owns the view state
    // while compose mode is active, the draft updates through it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [composeMode, openPath]);

  const composeEdit = useCallback(
    (key: string, value: string) => {
      setComposeFile((current) => {
        if (current === null) return current;
        const lines = setPropertyValue(current.lines, key, value);
        setDraft(serializeProperties(lines));
        return { ...current, lines };
      });
    },
    [],
  );

  // §34: the registry decides on file open whether a specialized editor
  // claims this file. The decision is per FILE, not per keystroke — a
  // draft passing through an empty title mid-edit must not yank the
  // mode away — so the content sniff reads the path and the draft only
  // when the file changes.
  const specialized = useMemo(
    () => specializedEditorFor(openPath, draft),
    // draft deliberately read once per file: detection is a property of
    // the file on disk.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [openPath],
  );

  // The specialized editors edit through the SAME row model: a
  // transform over the lines, serialized back to the draft. One
  // pipeline, three faces (Source, Compose, specialized).
  const composeApply = useCallback(
    (transform: (lines: PropertyLine[]) => PropertyLine[]) => {
      setComposeFile((current) => {
        if (current === null) return current;
        const lines = transform(current.lines);
        setDraft(serializeProperties(lines));
        return { ...current, lines };
      });
    },
    [],
  );

  const scoreboardSpec = useMemo(
    () =>
      specialized?.id === "scoreboard" && composeFile !== null
        ? readScoreboard(composeFile.lines)
        : null,
    [specialized, composeFile],
  );

  // A different file restarts the mode choice: the specialized tab is a
  // claim about THIS file, and it does not travel to the next one.
  useEffect(() => {
    setSpecializedMode(false);
  }, [openPath]);

  // Save, then the confirmed restart: two promises, one honest order —
  // the bytes are on the daemon before anything asks a player to wait.
  const saveAndRestart = useCallback(() => {
    if (openPath === null) return;
    if (!window.confirm(`Restart ${serverId} now? The new settings apply at boot, and players will be disconnected.`)) {
      return;
    }
    setBusy(true);
    setError(null);
    const bytes = new TextEncoder().encode(draft);
    void writeWholeFile(serverId, openPath, bytes)
      .then(() => {
        setSaved(draft);
        return restartServer(serverId);
      })
      .then(() => refresh(dir))
      .catch((cause: unknown) => {
        const described = describeError(cause);
        setError({ message: `described.title`, code: described.code });
      })
      .finally(() => setBusy(false));
  }, [serverId, openPath, draft, dir, refresh]);

  // The listing renders in slices: the first commit paints the window,
  // the rest lands over idle frames (see ui/deferred.ts). A refreshed
  // listing (new directory, post-action refresh) resets it for free.
  const entries = listing?.entries ?? NO_ENTRIES;
  const deferred = useDeferredWindow(entries, dir);

  return (
    <section className={styles.files} aria-label="Server files">
      <nav className={styles.crumbs} aria-label="Path">
        <button className={styles.crumb} onClick={() => { setOpenPath(null); refresh(""); }}>
          root
        </button>
        {crumbs.map((crumb) => (
          <span key={crumb.path} className={styles.crumbGap}>
            <span className={styles.separator}>/</span>
            <button
              className={styles.crumb}
              onClick={() => { setOpenPath(null); refresh(crumb.path); }}
            >
              {crumb.name}
            </button>
          </span>
        ))}
      </nav>

      {error ? (
        <div className={styles.alert} role="alert">
          <span>{error.message}</span>
          {error.code ? <span className={styles.codeChip}>{error.code}</span> : null}
        </div>
      ) : null}

      {openPath !== null ? (
        <div className={styles.editor}>
          <div className={styles.editorBar}>
            <span className={styles.editorPath}>{openPath}</span>
            <span className={dirty ? styles.dirty : styles.clean}>
              {dirty ? "unsaved changes" : "saved"}
            </span>
            {openPath.toLowerCase().endsWith(".properties") ? (
              <div className={styles.modeSwitch} role="tablist" aria-label="Editor mode">
                <button
                  role="tab"
                  aria-selected={!composeMode}
                  className={composeMode ? styles.modeTab : styles.modeTabActive}
                  onClick={() => setComposeMode(false)}
                >
                  Source
                </button>
                <button
                  role="tab"
                  aria-selected={composeMode && !specializedMode}
                  className={
                    composeMode && !specializedMode ? styles.modeTabActive : styles.modeTab
                  }
                  onClick={() => setComposeMode(true)}
                >
                  Compose
                </button>
                {specialized !== null ? (
                  <button
                    role="tab"
                    aria-selected={composeMode && specializedMode}
                    className={
                      composeMode && specializedMode ? styles.modeTabActive : styles.modeTab
                    }
                    onClick={() => {
                      setComposeMode(true);
                      setSpecializedMode(true);
                    }}
                  >
                    {specialized.label}
                  </button>
                ) : null}
              </div>
            ) : null}
            {plan.explain ? (
              <span className={styles.reloadNote} title={plan.explain}>
                {plan.explain}
              </span>
            ) : null}
            <Button variant="primary" onClick={save} disabled={busy || !dirty}>
              Save
            </Button>
            {plan.restartable && running ? (
              <Button variant="primary" onClick={saveAndRestart} disabled={busy}>
                Save &amp; Restart
              </Button>
            ) : null}
            <Button onClick={() => { setOpenPath(null); setComposeMode(false); setSpecializedMode(false); }}>Close</Button>
          </div>
          {composeMode && specializedMode && specialized?.id === "scoreboard" && scoreboardSpec !== null ? (
            <ScoreboardEditor
              spec={scoreboardSpec}
              onTitle={(title) => composeApply((lines) => writeScoreboardTitle(lines, title))}
              onRows={(rows) => composeApply((lines) => writeScoreboardRows(lines, scoreboardSpec, rows))}
            />
          ) : composeMode && composeFile !== null ? (
            <div className={styles.compose} aria-label={`Compose view for ${openPath}`}>
              {composeFile.lines
                .filter((line) => line.kind === "pair")
                .map((line) => {
                  const control = controlFor(line.value ?? "");
                  const label = humanizeKey(line.key ?? "");
                  return (
                    <div key={line.key} className={styles.composeRow}>
                      <label className={styles.composeLabel} htmlFor={`compose-${line.key}`}>
                        {label}
                        <span className={styles.composeKey}>{line.key}</span>
                      </label>
                      {control === "boolean" ? (
                        <button
                          id={`compose-${line.key}`}
                          type="button"
                          role="switch"
                          aria-checked={(line.value ?? "") === "true"}
                          className={styles.toggle}
                          onClick={() =>
                            composeEdit(line.key ?? "", (line.value ?? "") === "true" ? "false" : "true")
                          }
                        >
                          {(line.value ?? "") === "true" ? "ON" : "OFF"}
                        </button>
                      ) : control === "number" ? (
                        <input
                          id={`compose-${line.key}`}
                          type="number"
                          className={styles.composeInput}
                          value={line.value ?? ""}
                          onChange={(event) => composeEdit(line.key ?? "", event.target.value)}
                        />
                      ) : (
                        <input
                          id={`compose-${line.key}`}
                          type="text"
                          className={styles.composeInput}
                          value={line.value ?? ""}
                          onChange={(event) => composeEdit(line.key ?? "", event.target.value)}
                        />
                      )}
                    </div>
                  );
                })}
              {composeFile.lines.some((line) => line.kind !== "pair") ? (
                <p className={styles.composeNote}>
                  Comments and blank lines are preserved exactly — Source shows and edits them.
                </p>
              ) : null}
            </div>
          ) : (
            <textarea
              className={styles.editorArea}
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              spellCheck={false}
              aria-label={`Editing ${openPath}`}
            />
          )}
        </div>
      ) : (
        <>
          <div className={styles.toolbar}>
            <Button
              disabled={busy || dir === "" || dir === "."}
              onClick={() => { setOpenPath(null); refresh(parentOf(dir)); }}
            >
              Up
            </Button>
            <Button
              disabled={busy}
              onClick={() => {
                const name = window.prompt("New folder name (inside the current directory):");
                if (name) act(() => mkdir(serverId, join(dir, name)), "mkdir failed");
              }}
            >
              New folder
            </Button>
            <Button
              disabled={busy}
              onClick={() => {
                const name = window.prompt("New empty file name:");
                if (name) act(() => writeWholeFile(serverId, join(dir, name), new Uint8Array()), "create failed");
              }}
            >
              New file
            </Button>
            <Button disabled={busy} onClick={() => uploadInput.current?.click()}>
              Upload
            </Button>
            <input
              ref={uploadInput}
              type="file"
              multiple
              hidden
              aria-label="Upload files into this directory"
              onChange={(event) => {
                if (event.target.files) uploadFiles(event.target.files);
                event.target.value = "";
              }}
            />
            <input
              className={styles.search}
              type="search"
              placeholder="Search the whole server…"
              value={searchBox}
              onChange={(event) => setSearchBox(event.target.value)}
              aria-label="Search file names across the server root"
            />
          </div>

          {searchQuery !== "" ? (
            <div className={styles.tableCard}>
              {searching ? (
                <p className={styles.emptyDir}>Searching "{searchQuery}"…</p>
              ) : searchResult && searchResult.hits.length > 0 ? (
                <table className={styles.table}>
                  <thead>
                    <tr>
                      <th scope="col">Name</th>
                      <th scope="col">Size</th>
                      <th scope="col">Where</th>
                    </tr>
                  </thead>
                  <tbody>
                    {searchResult.hits.map((hit) => {
                      const name = hit.path.slice(hit.path.lastIndexOf("/") + 1);
                      const parent = hit.path.slice(0, hit.path.lastIndexOf("/"));
                      return (
                        <tr key={hit.path}>
                          <td>
                            <button
                              className={styles.name}
                              onClick={() => {
                                if (hit.kind === "file") {
                                  setOpenPath(null);
                                  openFile(hit.path, hit.sizeBytes);
                                } else {
                                  setSearchBox("");
                                  setOpenPath(null);
                                  refresh(hit.path);
                                }
                              }}
                            >
                              {hit.kind === "directory" ? `${name}/` : name}
                            </button>
                          </td>
                          <td className={styles.num}>{formatSize(hit.sizeBytes)}</td>
                          <td className={styles.num}>{parent === "" ? "root" : parent}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              ) : (
                <p className={styles.emptyDir}>Nothing matches "{searchQuery}".</p>
              )}
              {searchResult?.truncated ? (
                <p className={styles.moreNote}>
                  The walk stopped at {searchResult.hits.length} matches after checking{" "}
                  {searchResult.scanned} entries — refine the query to see the rest.
                </p>
              ) : null}
            </div>
          ) : (
          <div className={styles.tableCard}>
            <table className={styles.table}>
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Size</th>
                <th scope="col">Modified</th>
                <th scope="col">
                  <span className={styles.visuallyHidden}>Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {loading ? (
                <tr>
                  <td colSpan={4}>Loading…</td>
                </tr>
              ) : (
                deferred.visible.map((entry) => {
                  const path = join(dir, entry.name);
                  return (
                    <tr key={entry.name} className={entry.symlinkOutside ? styles.denied : undefined}>
                      <td>
                        <button
                          className={styles.name}
                          onClick={() => openDirEntry(entry)}
                          title={
                            entry.symlinkOutside
                              ? "This link leaves the server root; ZIM will not follow it"
                              : entry.kind === "directory"
                                ? "Open directory"
                                : "Open in editor"
                          }
                        >
                          {entry.kind === "directory" ? `${entry.name}/` : entry.name}
                          {entry.symlinkOutside ? (
                            <span className={styles.badge}>outside root</span>
                          ) : null}
                        </button>
                      </td>
                      <td className={styles.num}>{formatSize(entry.sizeBytes)}</td>
                      <td className={styles.num}>{formatWhen(entry.modifiedMs)}</td>
                      <td className={styles.rowActions}>
                        <button
                          className={styles.rowButton}
                          disabled={busy}
                          onClick={() => {
                            const next = window.prompt(`Rename ${entry.name} to:`);
                            if (next && next !== entry.name) {
                              act(() => renameEntry(serverId, path, join(dir, next)), "rename failed");
                            }
                          }}
                        >
                          rename
                        </button>
                        <button
                          className={styles.rowButton}
                          disabled={busy}
                          onClick={() => copyEntry(entry)}
                        >
                          copy
                        </button>
                        <button
                          className={styles.rowButton}
                          disabled={busy}
                          onClick={() => moveEntry(entry)}
                        >
                          move
                        </button>
                        {entry.kind === "file" ? (
                          <button
                            className={styles.rowButton}
                            disabled={busy}
                            onClick={() => downloadEntry(entry)}
                          >
                            download
                          </button>
                        ) : null}
                        <button
                          className={styles.rowButton}
                          disabled={busy}
                          onClick={() => {
                            if (window.confirm(`Delete ${path}? This cannot be undone.`)) {
                              act(() => deleteEntry(serverId, path), "delete failed");
                            }
                          }}
                        >
                          delete
                        </button>
                      </td>
                    </tr>
                  );
                })
              )}
              {listing && listing.entries.length === 0 && !loading ? (
                <tr>
                  <td colSpan={4} className={styles.emptyDir}>
                    This directory is empty.
                  </td>
                </tr>
              ) : null}
            </tbody>
            </table>
          </div>
          )}
          {!loading && !deferred.done ? (
            <p className={styles.moreNote}>
              Showing {deferred.visible.length} of {deferred.total} — the rest render as the
              browser breathes.{" "}
              <button className={styles.noteButton} onClick={deferred.showAll}>
                Show all now
              </button>
            </p>
          ) : null}
          {listing && listing.total > listing.entries.length ? (
            <p className={styles.moreNote}>
              {listing.total - listing.entries.length} more entries — open subdirectories to
              browse them.
            </p>
          ) : null}
        </>
      )}
    </section>
  );
}
