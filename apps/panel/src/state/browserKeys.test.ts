// The browser keyboard contract's conformance table (ADR-0032): each
// case names the Chromium IDC it mirrors. The decision is pure — the
// same event means the same command regardless of which store executes
// it — and the text-field rules follow Chromium's: commands that type
// (Ctrl+C, letters, Home/End) stay the editor's; commands that navigate
// (Ctrl+T, Ctrl+W, Ctrl+1..9, Ctrl+L) stay the browser's.

import { describe, expect, it } from "vitest";
import { handleBrowserKey, type BrowserKeyApi } from "./browserKeys";

function api(): BrowserKeyApi & { calls: string[] } {
  const calls: string[] = [];
  const record = (name: string) => () => calls.push(name);
  return {
    calls,
    newTab: record("newTab"),
    reopenClosedTab: record("reopenClosedTab"),
    closeActiveTab: record("closeActiveTab"),
    cycleTab: (step) => calls.push(`cycleTab:${step}`),
    selectTabIndex: (index) => calls.push(`selectTabIndex:${index}`),
    focusAddressBar: record("focusAddressBar"),
    reload: record("reload"),
    goBack: record("goBack"),
    goForward: record("goForward"),
    bookmarkActive: record("bookmarkActive"),
    toggleBookmarksBar: record("toggleBookmarksBar"),
    togglePalette: record("togglePalette"),
    paletteOpen: () => false,
    openDevTools: record("openDevTools"),
  };
}

function keyEvent(over: {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
}): Parameters<typeof handleBrowserKey>[0] {
  return {
    key: over.key,
    ctrlKey: over.ctrlKey ?? false,
    metaKey: over.metaKey ?? false,
    shiftKey: over.shiftKey ?? false,
    altKey: over.altKey ?? false,
    defaultPrevented: false,
  };
}

const notAnInput = { isContentEditable: false, tagIsInput: false };
const anInput = { isContentEditable: false, tagIsInput: true };

describe("the browser keyboard contract (ADR-0032, Chromium IDC table)", () => {
  it("IDC_NEW_TAB — Ctrl+T opens a new-tab page", () => {
    const a = api();
    expect(handleBrowserKey(keyEvent({ key: "t", ctrlKey: true }), notAnInput, a)).toEqual({
      handled: true,
    });
    expect(a.calls).toEqual(["newTab"]);
  });

  it("IDC_REOPEN_CLOSED_TAB — Ctrl+Shift+T", () => {
    const a = api();
    expect(handleBrowserKey(keyEvent({ key: "T", ctrlKey: true, shiftKey: true }), notAnInput, a)).toEqual({ handled: true });
    expect(a.calls).toEqual(["reopenClosedTab"]);
  });

  it("IDC_CLOSE_TAB — Ctrl+W and Ctrl+F4", () => {
    const a = api();
    handleBrowserKey(keyEvent({ key: "w", ctrlKey: true }), notAnInput, a);
    handleBrowserKey(keyEvent({ key: "F4", ctrlKey: true }), notAnInput, a);
    expect(a.calls).toEqual(["closeActiveTab", "closeActiveTab"]);
  });

  it("IDC_SELECT_NEXT/PREVIOUS_TAB — Ctrl+Tab / Ctrl+Shift+Tab", () => {
    const a = api();
    handleBrowserKey(keyEvent({ key: "Tab", ctrlKey: true }), notAnInput, a);
    handleBrowserKey(keyEvent({ key: "Tab", ctrlKey: true, shiftKey: true }), notAnInput, a);
    expect(a.calls).toEqual(["cycleTab:1", "cycleTab:-1"]);
  });

  it("IDC_SELECT_TAB_0..7 and IDC_SELECT_LAST_TAB — Ctrl+1..8, Ctrl+9", () => {
    const a = api();
    for (const n of ["1", "3", "8"]) {
      handleBrowserKey(keyEvent({ key: n, ctrlKey: true }), notAnInput, a);
    }
    handleBrowserKey(keyEvent({ key: "9", ctrlKey: true }), notAnInput, a);
    expect(a.calls).toEqual(["selectTabIndex:0", "selectTabIndex:2", "selectTabIndex:7", "selectTabIndex:last"]);
  });

  it("IDC_FOCUS_LOCATION — Ctrl+L, F6, and Alt+D, even inside a field", () => {
    const a = api();
    handleBrowserKey(keyEvent({ key: "l", ctrlKey: true }), anInput, a);
    handleBrowserKey(keyEvent({ key: "F6" }), anInput, a);
    handleBrowserKey(keyEvent({ key: "d", altKey: true }), anInput, a);
    expect(a.calls).toEqual(["focusAddressBar", "focusAddressBar", "focusAddressBar"]);
  });

  it("IDC_RELOAD — Ctrl+R and F5", () => {
    const a = api();
    handleBrowserKey(keyEvent({ key: "r", ctrlKey: true }), notAnInput, a);
    handleBrowserKey(keyEvent({ key: "F5" }), notAnInput, a);
    expect(a.calls).toEqual(["reload", "reload"]);
  });

  it("IDC_BACK / IDC_FORWARD — Alt+Left / Alt+Right", () => {
    const a = api();
    handleBrowserKey(keyEvent({ key: "ArrowLeft", altKey: true }), notAnInput, a);
    handleBrowserKey(keyEvent({ key: "ArrowRight", altKey: true }), notAnInput, a);
    expect(a.calls).toEqual(["goBack", "goForward"]);
  });

  it("IDC_BOOKMARK_PAGE — Ctrl+D bookmarks, but Ctrl+D inside an input stays the editor's", () => {
    const a = api();
    handleBrowserKey(keyEvent({ key: "d", ctrlKey: true }), notAnInput, a);
    expect(a.calls).toEqual(["bookmarkActive"]);
    const b = api();
    expect(handleBrowserKey(keyEvent({ key: "d", ctrlKey: true }), anInput, b).handled).toBe(false);
    expect(b.calls).toEqual([]);
  });

  it("IDC_SHOW_BOOKMARK_BAR — Ctrl+Shift+B", () => {
    const a = api();
    handleBrowserKey(keyEvent({ key: "B", ctrlKey: true, shiftKey: true }), notAnInput, a);
    expect(a.calls).toEqual(["toggleBookmarksBar"]);
  });

  it("IDC_DEV_TOOLS — F12 and Ctrl+Shift+I, even inside a field; plain I stays the editor's", () => {
    const a = api();
    handleBrowserKey(keyEvent({ key: "F12" }), notAnInput, a);
    handleBrowserKey(keyEvent({ key: "I", ctrlKey: true, shiftKey: true }), anInput, a);
    expect(a.calls).toEqual(["openDevTools", "openDevTools"]);
    const b = api();
    expect(handleBrowserKey(keyEvent({ key: "i" }), anInput, b).handled).toBe(false);
    expect(b.calls).toEqual([]);
  });

  it("IDC_STOP — Escape closes the palette only when the palette owns the key", () => {
    const closed = api();
    expect(handleBrowserKey(keyEvent({ key: "Escape" }), notAnInput, closed).handled).toBe(false);
    const open = api();
    open.paletteOpen = () => true;
    expect(handleBrowserKey(keyEvent({ key: "Escape" }), notAnInput, open)).toEqual({
      handled: true,
    });
    expect(open.calls).toEqual(["togglePalette"]);
  });

  it("typing stays the editor's: letters and Home/End are never commands", () => {
    const a = api();
    for (const key of ["a", "d", "w", "Home", "End", "ArrowUp"]) {
      expect(handleBrowserKey(keyEvent({ key }), anInput, a).handled).toBe(false);
    }
    expect(a.calls).toEqual([]);
  });
});
