// The browser keyboard contract (ADR-0032): Chromium's command table
// (chrome/app/chrome_command_ids.h) ported as behavior, not code. Each
// entry names the IDC it implements so the conformance test can be read
// against the upstream table; the destination model beneath it stays
// ZaminPanel's (§58) — Ctrl+T opens a new-tab page, not a web page.
//
// This module is pure: it decides what a key event MEANS and returns a
// verb; the caller (App's window listener) executes it against the
// stores. Tests assert the decision table, including the modifiers and
// the "type in an input" rules Chromium keeps (Ctrl+L, Ctrl+C copy,
// Escape stop must not be swallowed while a field holds focus).

export interface BrowserKeyApi {
  newTab(): void;
  reopenClosedTab(): void;
  closeActiveTab(): void;
  cycleTab(step: 1 | -1): void;
  selectTabIndex(index: number | "last"): void;
  focusAddressBar(): void;
  reload(): void;
  goBack(): void;
  goForward(): void;
  bookmarkActive(): void;
  toggleBookmarksBar(): void;
  togglePalette(): void;
  paletteOpen(): boolean;
}

export type BrowserKeyVerdict =
  | { handled: true }
  | { handled: false };

/** Chromium's rule: single-letter keys and Home/End stay the editor's
 *  inside a text field; commands that don't type (Ctrl+T, Ctrl+W…) stay
 *  the browser's. The address bar's Ctrl+L works everywhere (Chromium
 *  IDC_FOCUS_LOCATION). */
export function handleBrowserKey(
  event: {
    key: string;
    ctrlKey: boolean;
    metaKey: boolean;
    shiftKey: boolean;
    altKey: boolean;
    defaultPrevented: boolean;
  },
  target: { isContentEditable: boolean; tagIsInput: boolean },
  api: BrowserKeyApi,
): BrowserKeyVerdict {
  const mod = event.ctrlKey || event.metaKey;
  const key = event.key;
  const lower = key.toLowerCase();

  if (mod && !event.shiftKey && !event.altKey && lower === "k") {
    // IDC_FOCUS_PAGE_ACTIONS… the command palette is ZaminPanel's own
    // mapping of "the browser's command surface" (§50); the founder's
    // Ctrl+K keeps it.
    api.togglePalette();
    return { handled: true };
  }

  if (mod && event.shiftKey && !event.altKey && lower === "t") {
    // IDC_REOPEN_CLOSED_TAB (Chromium: Ctrl+Shift+T).
    api.reopenClosedTab();
    return { handled: true };
  }
  if (mod && !event.shiftKey && !event.altKey && lower === "t") {
    // IDC_NEW_TAB (Ctrl+T) — a ZaminPanel new-tab page (§6).
    api.newTab();
    return { handled: true };
  }
  if ((mod && !event.shiftKey && !event.altKey && lower === "w") ||
      (mod && key === "F4")) {
    // IDC_CLOSE_TAB (Ctrl+W) / (Ctrl+F4).
    api.closeActiveTab();
    return { handled: true };
  }
  if (mod && key === "Tab") {
    // IDC_SELECT_NEXT_TAB / IDC_SELECT_PREVIOUS_TAB (Ctrl+Tab /
    // Ctrl+Shift+Tab).
    api.cycleTab(event.shiftKey ? -1 : 1);
    return { handled: true };
  }
  if (mod && !event.shiftKey && !event.altKey && /^[1-9]$/.test(key)) {
    // IDC_SELECT_TAB_0..7 and IDC_SELECT_LAST_TAB (Ctrl+9 = last).
    api.selectTabIndex(key === "9" ? "last" : Number(key) - 1);
    return { handled: true };
  }
  if (mod && !event.shiftKey && !event.altKey && lower === "l") {
    // IDC_FOCUS_LOCATION (Ctrl+L) — the omnibox focus trio's first chord.
    api.focusAddressBar();
    return { handled: true };
  }
  if (key === "F6") {
    // IDC_FOCUS_LOCATION (F6) — no modifier, Chromium's own mapping.
    api.focusAddressBar();
    return { handled: true };
  }
  if (event.altKey && !mod && key === "d") {
    // IDC_FOCUS_LOCATION via Alt+D — same command, third chord.
    api.focusAddressBar();
    return { handled: true };
  }
  if ((mod && !event.shiftKey && !event.altKey && lower === "r") || key === "F5") {
    // IDC_RELOAD (Ctrl+R, F5) — §60's reload: the view, never the
    // server process.
    api.reload();
    return { handled: true };
  }
  if (event.altKey && !mod && key === "ArrowLeft") {
    // IDC_BACK (Alt+Left).
    api.goBack();
    return { handled: true };
  }
  if (event.altKey && !mod && key === "ArrowRight") {
    // IDC_FORWARD (Alt+Right).
    api.goForward();
    return { handled: true };
  }
  if (mod && event.shiftKey && !event.altKey && lower === "b") {
    // IDC_SHOW_BOOKMARK_BAR (Ctrl+Shift+B) — §55's bar toggle.
    api.toggleBookmarksBar();
    return { handled: true };
  }
  if (mod && !event.shiftKey && !event.altKey && lower === "d" && !target.tagIsInput) {
    // IDC_BOOKMARK_PAGE (Ctrl+D) — bookmark the active destination.
    api.bookmarkActive();
    return { handled: true };
  }
  if (event.key === "Escape" && !mod && !event.altKey) {
    // IDC_STOP (Escape) — the palette's close verb is the same idea:
    // stop what the browser surface is doing. Only when it is open, so
    // a text field's Escape (dismiss suggestion, close its own menu)
    // stays the field's (Chromium's rule).
    if (api.paletteOpen()) {
      api.togglePalette();
      return { handled: true };
    }
    return { handled: false };
  }
  return { handled: false };
}
