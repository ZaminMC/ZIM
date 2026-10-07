// File browser + editor: the server root over the wire (spec §8). The
// listing pages directories-first; a text file opens in the editor and
// saves through the staged upload (chunked write → one atomic commit), so
// a crashed tab can never leave a half-written file at the target path.

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  deleteEntry,
  listFiles,
  mkdir,
  readWholeFile,
  renameEntry,
  writeWholeFile,
} from "../state/actions";
import type { FilesEntry, FilesListResult } from "../protocol/types";
import { describeError } from "../state/errors";
import { Button } from "../ui/Button";
import { useDeferredWindow } from "../ui/deferred";
import styles from "./FilesView.module.css";

const EDIT_LIMIT_BYTES = 1024 * 1024; // the editor is for text files

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
          message: `${entry.name} is a symlink outside the server root — the daemon will not follow it.`,
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
          setError({ message: `${note}: ${described.title}`, code: described.code });
        })
        .finally(() => setBusy(false));
    },
    [dir, refresh],
  );

  const dirty = draft !== saved;

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
            <Button variant="primary" onClick={save} disabled={busy || !dirty}>
              Save
            </Button>
            <Button onClick={() => setOpenPath(null)}>Close</Button>
          </div>
          <textarea
            className={styles.editorArea}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            spellCheck={false}
            aria-label={`Editing ${openPath}`}
          />
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
          </div>

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
                              ? "This link leaves the server root; the daemon will not follow it"
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
