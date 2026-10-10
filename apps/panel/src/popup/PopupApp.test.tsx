// The popup overlay's contract tests. The overlay is the shell's
// application-owned menu surface — the thing that replaced the OS prompt
// (`tauri.localhost says: …`) and the native gray context menu — so its
// behavior is pinned here the way the security boundary demands: which
// verbs each shape offers, the group form's validation law (an empty name
// is an error, never a silent group), the Escape layering (form cancels
// first, popup second), the arrow-key rove, and the zoom row's bounds.
//
// The Tauri lane is exercised for real: `window.__TAURI_INTERNALS__` is
// stubbed and `@tauri-apps/api/core` is module-mocked, so the tests
// assert on the actual `invoke("shell_command", …)` calls the overlay
// makes — not on a demo stand-in's console echoes.

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { PopupApp } from "./PopupApp";
import { CMD } from "../commandIds";

// The host seam: shell_popup_boot answers the overlay's context; every
// other verb is recorded for the assertions.
const bootState: { context: Record<string, unknown> | null } = { context: null };
let invoked: Array<{ cmd: string; args: Record<string, unknown> | undefined }> = [];

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    invoked.push({ cmd, args });
    if (cmd === "shell_popup_boot") {
      return bootState.context === null
        ? Promise.reject(new Error("the popup died before its boot answered"))
        : Promise.resolve(bootState.context);
    }
    return Promise.resolve(null);
  },
}));

const TAURI_CONTEXT = {
  kind: "tab-menu",
  tab_id: 7,
  pinned: false,
  muted: false,
  zoom: 1,
  bar_visible: false,
};

function tauriLane(): void {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
}

function bootContext(context: Record<string, unknown> | null): void {
  bootState.context = context;
}

async function renderBooted(context: Record<string, unknown> = TAURI_CONTEXT) {
  tauriLane();
  bootContext(context);
  const view = render(<PopupApp />);
  await waitFor(() => {
    expect(screen.queryByRole("menu")).not.toBeNull();
  });
  return view;
}

describe("<PopupApp /> boot", () => {
  beforeEach(() => {
    invoked = [];
    bootContext(null);
    history.replaceState(null, "", "/popup.html");
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  afterEach(() => {
    cleanup();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("boots from shell_popup_boot and renders the tab menu", async () => {
    await renderBooted();
    expect(screen.getByRole("menuitem", { name: /Pin tab/ })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: /Add to new group/ })).toBeTruthy();
  });

  it("a popup whose boot dies renders only the gutter — no crash, no menu", async () => {
    tauriLane();
    bootContext(null);
    const { container } = render(<PopupApp />);
    await waitFor(() => {
      // The boot invoke happened and rejected; the overlay stays a
      // silent gutter for the host to tear down.
      expect(invoked.some((call) => call.cmd === "shell_popup_boot")).toBe(true);
    });
    expect(container.querySelector(".menu")).toBeNull();
  });

  it("the demo lane reads its kind from the query, no host involved", async () => {
    history.replaceState(null, "", "/popup.html?demo&kind=app-menu");
    const { container } = render(<PopupApp />);
    await waitFor(() => {
      expect(container.querySelector(".menu")).not.toBeNull();
    });
    // The app menu's own headline verb, with its keyboard hint.
    expect(screen.getByRole("menuitem", { name: /New window/ })).toBeTruthy();
    // The demo lane never touched the host.
    expect(invoked).toHaveLength(0);
  });
});

describe("<PopupApp /> the tab menu", () => {
  beforeEach(() => {
    invoked = [];
    bootContext(null);
    history.replaceState(null, "", "/popup.html");
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  afterEach(() => {
    cleanup();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("offers Chromium's close-context set", async () => {
    await renderBooted();
    // Exact names where one verb is a prefix of another ("Close tab"
    // vs "Close other tabs" vs "Close tabs to the right"); the group
    // verb carries a "›" hint in its accessible name, so it matches
    // by its anchored prefix instead.
    const labels = [
      "New tab to the right",
      "Pin tab",
      "Mute tab",
      "Duplicate",
    ];
    for (const label of labels) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
    expect(screen.getByRole("menuitem", { name: /^Add to new group…/ })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: /^Close tab$/ })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: /^Close other tabs$/ })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: /^Close tabs to the right$/ })).toBeTruthy();
  });

  it("flips the pin verb with the tab's posture", async () => {
    await renderBooted({ ...TAURI_CONTEXT, pinned: true });
    expect(screen.getByRole("menuitem", { name: /Unpin tab/ })).toBeTruthy();
    expect(screen.queryByRole("menuitem", { name: /^Pin tab$/ })).toBeNull();
  });

  it("the close item speaks the selection's plural and commands its scope", async () => {
    // A selected context tab: the menu says "Close N tabs" (tab_menu_model.cc's
    // IDS_TAB_CXMENU_CLOSETAB plural) and the command rides
    // CLOSE_SELECTED_TABS — GetIndicesForCommand's whole-selection scope.
    await renderBooted({ ...TAURI_CONTEXT, tab_selected: true, selection_size: 3 });
    expect(screen.getByRole("menuitem", { name: /^Close 3 tabs$/ })).toBeTruthy();
    expect(screen.queryByRole("menuitem", { name: /^Close tab$/ })).toBeNull();
    fireEvent.click(screen.getByRole("menuitem", { name: /^Close 3 tabs$/ }));
    await waitFor(() => {
      const close = invoked.find(
        (call) => call.args?.id === CMD.CLOSE_SELECTED_TABS,
      );
      expect(close).toBeTruthy();
      expect(close?.args).toEqual({ id: CMD.CLOSE_SELECTED_TABS, arg: { tab_id: 7 } });
    });
    // An unselected context tab keeps the singular and the one-tab verb.
    cleanup();
    await renderBooted({ ...TAURI_CONTEXT, tab_selected: false, selection_size: 3 });
    expect(screen.getByRole("menuitem", { name: /^Close tab$/ })).toBeTruthy();
    expect(screen.queryByRole("menuitem", { name: /^Close [0-9]/ })).toBeNull();
  });

  it("flips the mute verb with the tab's posture", async () => {
    await renderBooted({ ...TAURI_CONTEXT, muted: true });
    expect(screen.getByRole("menuitem", { name: /Unmute tab/ })).toBeTruthy();
  });

  it("labels the layout verb from the strip's axis (§54)", async () => {
    await renderBooted({ ...TAURI_CONTEXT, vertical: true });
    expect(screen.getByRole("menuitem", { name: /Use horizontal strip/ })).toBeTruthy();
  });

  it("dispatches the verb with the booted tab id, then closes", async () => {
    await renderBooted();
    fireEvent.click(screen.getByRole("menuitem", { name: /Duplicate/ }));
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_command")).toBe(true);
    });
    const command = invoked.find((c) => c.cmd === "shell_command");
    expect(command?.args).toEqual({ id: CMD.DUPLICATE_TAB, arg: { tab_id: 7 } });
    expect(invoked.some((c) => c.cmd === "shell_popup_close")).toBe(true);
  });
});

describe("<PopupApp /> the group form", () => {
  beforeEach(() => {
    invoked = [];
    bootContext(null);
    history.replaceState(null, "", "/popup.html");
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  afterEach(() => {
    cleanup();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  async function openForm() {
    const view = await renderBooted();
    fireEvent.click(screen.getByRole("menuitem", { name: /Add to new group/ }));
    await waitFor(() => {
      expect(screen.getByRole("dialog", { name: "Name the new group" })).toBeTruthy();
    });
    return view;
  }

  it("opens in place — the same overlay turns into the naming form", async () => {
    await openForm();
    expect(screen.getByRole("heading", { name: "Name the group" })).toBeTruthy();
    // The menu it came from is gone; one popup, no OS prompt anywhere.
    expect(screen.queryByRole("menuitem", { name: /Pin tab/ })).toBeNull();
  });

  it("autofocuses the input", async () => {
    await openForm();
    expect(document.activeElement?.tagName).toBe("INPUT");
  });

  it("an empty name is a validation error, never a silent group", async () => {
    await openForm();
    const input = screen.getByRole("textbox");
    fireEvent.keyDown(input, { key: "Enter" });
    const error = await waitFor(() => screen.getByRole("alert"));
    expect(error.textContent).toBe("Give the group a name.");
    // No command left the overlay.
    expect(invoked.some((c) => c.cmd === "shell_command")).toBe(false);
  });

  it("Enter confirms: ADD_NEW_TAB_TO_GROUP with the tab id and the trimmed label", async () => {
    await openForm();
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "  survival  " } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_command")).toBe(true);
    });
    const command = invoked.find((c) => c.cmd === "shell_command");
    expect(command?.args).toEqual({
      id: CMD.ADD_NEW_TAB_TO_GROUP,
      arg: { tab_id: 7, label: "survival" },
    });
    expect(invoked.some((c) => c.cmd === "shell_popup_close")).toBe(true);
  });

  it("Create group confirms the same way", async () => {
    await openForm();
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "creative" } });
    fireEvent.click(screen.getByRole("button", { name: "Create group" }));
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_command")).toBe(true);
    });
    const command = invoked.find((c) => c.cmd === "shell_command");
    expect(command?.args).toEqual({
      id: CMD.ADD_NEW_TAB_TO_GROUP,
      arg: { tab_id: 7, label: "creative" },
    });
  });

  it("Escape cancels the form back to the menu it came from — the popup stays", async () => {
    await openForm();
    fireEvent.keyDown(window, { key: "Escape" });
    // The menu is back…
    expect(screen.getByRole("menuitem", { name: /Pin tab/ })).toBeTruthy();
    // …and the popup was NOT dismissed.
    expect(invoked.some((c) => c.cmd === "shell_popup_close")).toBe(false);
  });

  it("the Create button is disabled while the name is empty", async () => {
    await openForm();
    const create = screen.getByRole<HTMLButtonElement>("button", { name: "Create group" });
    expect(create.disabled).toBe(true);
  });
});

describe("<PopupApp /> the group editor", () => {
  beforeEach(() => {
    invoked = [];
    bootContext(null);
    history.replaceState(null, "", "/popup.html");
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  afterEach(() => {
    cleanup();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  const EDITOR_CONTEXT = {
    kind: "group-editor",
    tab_id: null,
    group_id: 2,
    label: "survival",
    color: 1,
    collapsed: false,
  };

  const EDITOR_ITEMS = ["New tab in group", "Ungroup", "Close group"];

  async function openEditor(context: Record<string, unknown> = EDITOR_CONTEXT) {
    tauriLane();
    bootContext(context);
    const view = render(<PopupApp />);
    await waitFor(() => {
      expect(screen.getByRole("dialog", { name: "Edit group" })).toBeTruthy();
    });
    return view;
  }

  it("carries the title, the nine-color grid, and the three rows", async () => {
    await openEditor();
    expect(screen.getByRole("textbox", { name: "Group name" })).toBeTruthy();
    const radios = screen.getAllByRole("radio");
    expect(radios).toHaveLength(9); // kNumEntries, wire order
    expect(radios[1]!.getAttribute("aria-checked")).toBe("true"); // blue booted
    for (const label of EDITOR_ITEMS) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
  });

  it("renames LIVE — every keystroke reaches the model, the editor stays open", async () => {
    await openEditor();
    const input = screen.getByRole("textbox", { name: "Group name" });
    fireEvent.change(input, { target: { value: "surv" } });
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_command")).toBe(true);
    });
    const command = invoked.find((c) => c.cmd === "shell_command");
    expect(command?.args).toEqual({ id: CMD.RENAME_GROUP, arg: { group_id: 2, label: "surv" } });
    // The write is in-place: no shell_popup_close rode along.
    expect(invoked.some((c) => c.cmd === "shell_popup_close")).toBe(false);
  });

  it("picks a color in place — SET_GROUP_COLOR, still open", async () => {
    await openEditor();
    fireEvent.click(screen.getByRole("radio", { name: "Cyan" }));
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_command")).toBe(true);
    });
    const command = invoked.find((c) => c.cmd === "shell_command");
    expect(command?.args).toEqual({ id: CMD.SET_GROUP_COLOR, arg: { group_id: 2, color: 7 } });
    expect(invoked.some((c) => c.cmd === "shell_popup_close")).toBe(false);
  });

  it("Ungroup frees the members and closes", async () => {
    await openEditor();
    fireEvent.click(screen.getByRole("menuitem", { name: "Ungroup" }));
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_command")).toBe(true);
    });
    const command = invoked.find((c) => c.cmd === "shell_command");
    expect(command?.args).toEqual({ id: CMD.UNGROUP_GROUP, arg: { group_id: 2 } });
    expect(invoked.some((c) => c.cmd === "shell_popup_close")).toBe(true);
  });

  it("Enter leaves (the rename already happened on the keystroke)", async () => {
    await openEditor();
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Group name" }), { key: "Enter" });
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_popup_close")).toBe(true);
    });
    expect(invoked.some((c) => c.cmd === "shell_command")).toBe(false);
  });

  it("the tab menu lists the strip's groups under the add verb", async () => {
    await renderBooted({
      ...TAURI_CONTEXT,
      groups: [
        { id: 1, label: "survival", color: 1 },
        { id: 2, label: "", color: 0 },
      ],
    });
    expect(screen.getByRole("menuitem", { name: /survival/ })).toBeTruthy();
    // An EMPTY label reads as the untitled group — the chip would be a
    // bare color dot, the menu needs words.
    expect(screen.getByRole("menuitem", { name: /Untitled group/ })).toBeTruthy();
  });

  it("Remove from group rides the tab's grouped posture", async () => {
    await renderBooted({ ...TAURI_CONTEXT, grouped: 1 });
    fireEvent.click(screen.getByRole("menuitem", { name: /Remove from group/ }));
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_command")).toBe(true);
    });
    const command = invoked.find((c) => c.cmd === "shell_command");
    expect(command?.args).toEqual({ id: CMD.REMOVE_TAB_FROM_GROUP, arg: { tab_id: 7 } });
  });
});

describe("<PopupApp /> the app menu", () => {
  beforeEach(() => {
    invoked = [];
    bootContext(null);
    history.replaceState(null, "", "/popup.html");
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  afterEach(() => {
    cleanup();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  async function renderAppMenu(context: Record<string, unknown> = {}) {
    return renderBooted({ kind: "app-menu", tab_id: null, ...context });
  }

  it("offers the browser-level actions — server management never enters", async () => {
    await renderAppMenu();
    const labels = [
      "New tab",
      "New window",
      "Bookmarks bar",
      "Developer tools",
      "Application logs",
      "Downloads",
      "Extensions",
      "Jobs",
      "Settings",
      "About ZIM",
    ];
    for (const label of labels) {
      expect(screen.getByRole("menuitem", { name: new RegExp(label) })).toBeTruthy();
    }
  });

  it("the Bookmarks bar item carries its checkmark from the window posture", async () => {
    const { container } = await renderAppMenu({ bar_visible: true });
    const item = screen.getByRole("menuitem", { name: /Bookmarks bar/ });
    const tick = item.querySelector(".tick");
    expect(tick?.getAttribute("aria-hidden")).toBeNull();
    expect(container).toBeTruthy();
  });

  it("reads the zoom posture and bounds the row", async () => {
    await renderAppMenu({ zoom: 1 });
    expect(screen.getByText("100%")).toBeTruthy();
    const reset = screen.getByRole<HTMLButtonElement>("button", { name: "Reset zoom" });
    expect(reset.disabled).toBe(true);
    const zoomIn = screen.getByRole<HTMLButtonElement>("button", { name: "Zoom in" });
    expect(zoomIn.disabled).toBe(false);
  });

  it("bounds the zoom row at both ends", async () => {
    await renderAppMenu({ zoom: 0.25 });
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Zoom out" }).disabled).toBe(true);

    cleanup();
    invoked = [];
    await renderAppMenu({ zoom: 5 });
    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Zoom in" }).disabled).toBe(true);
  });

  it("Developer tools navigates the active tab, then closes", async () => {
    await renderAppMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: /Developer tools/ }));
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_command")).toBe(true);
    });
    const command = invoked.find((c) => c.cmd === "shell_command");
    expect(command?.args).toEqual({
      id: CMD.NAVIGATE_ACTIVE,
      arg: { destination: { kind: "devtools" } },
    });
  });
});

describe("<PopupApp /> the keyboard contract", () => {
  beforeEach(() => {
    invoked = [];
    bootContext(null);
    history.replaceState(null, "", "/popup.html");
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  afterEach(() => {
    cleanup();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("ArrowDown roves focus to the next item, which then activates", async () => {
    await renderBooted();
    const items = screen.getAllByRole("menuitem");
    const first = items[0]!;
    // The second item is the group verb — its activation OPENS THE NAMING
    // FORM (tab_menu_model.cc's submenu law), it dispatches nothing. The
    // rove walks two steps to a dispatcher.
    first.focus();
    expect(document.activeElement).toBe(first);
    fireEvent.keyDown(window, { key: "ArrowDown" });
    expect(document.activeElement).toBe(items[1]!);
    fireEvent.keyDown(window, { key: "ArrowDown" });
    const third = items[2]!;
    expect(document.activeElement).toBe(third);
    // The roved-to item activates (the browser fires click on Enter for
    // the focused button — jsdom doesn't synthesize that native step, so
    // the activation half is asserted through the click it produces).
    fireEvent.click(third);
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_command")).toBe(true);
    });
  });

  it("ArrowUp wraps from the first item to the last", async () => {
    await renderBooted();
    const items = screen.getAllByRole("menuitem");
    const first = items[0]!;
    const last = items[items.length - 1]!;
    first.focus();
    fireEvent.keyDown(window, { key: "ArrowUp" });
    expect(document.activeElement).toBe(last);
  });

  it("Escape on the menu dismisses the popup", async () => {
    await renderBooted();
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(invoked.some((c) => c.cmd === "shell_popup_close")).toBe(true);
    });
  });
});

// -- The hover card ---------------------------------------------------------
// The shell's fifth widget shape: the frame drives it (show, slide, fade),
// this overlay only paints. The tests exercise the REAL tauri event lane:
// the mocked @tauri-apps/api/event records the listeners, the tests fire
// them — the same payloads shell_popup_update / shell_popup_fade carry.

const eventListeners: Array<{ event: string; handler: (e: { payload: unknown }) => void }> = [];

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: (e: { payload: unknown }) => void) => {
    eventListeners.push({ event, handler });
    return Promise.resolve(() => {});
  },
}));

const fireShellEvent = (event: string, payload?: unknown): void => {
  for (const listener of eventListeners) {
    if (listener.event === event) listener.handler({ payload });
  }
};

const HOVER_TAB_CARD = {
  kind: "tab",
  x: 40,
  y: 47,
  band: { x: 0, y: 47, w: 1024, h: 553 },
  title: "Server survival — console",
  domain: "server/survival",
  members: [],
  excess: 0,
};

const HOVER_GROUP_CARD = {
  kind: "group",
  x: 40,
  y: 47,
  band: { x: 0, y: 47, w: 1024, h: 553 },
  title: "survival (3 tabs)",
  domain: null,
  members: ["Server survival", "Console hub", "zamind — heartbeat"],
  excess: 2,
};

describe("<PopupApp /> the hover card", () => {
  beforeEach(() => {
    invoked = [];
    eventListeners.length = 0;
    bootContext(null);
    history.replaceState(null, "", "/popup.html");
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  afterEach(() => {
    cleanup();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  async function renderCard(card: Record<string, unknown>) {
    tauriLane();
    bootContext({ kind: "hover-card", tab_id: null, card });
    const view = render(<PopupApp />);
    await waitFor(() => {
      expect(view.container.querySelector(".hover-card")).not.toBeNull();
    });
    return view;
  }

  it("paints the tab card: title over the domain line, no menu, no gutter law", async () => {
    const { container } = await renderCard(HOVER_TAB_CARD);
    expect(container.querySelector(".menu")).toBeNull();
    expect(container.querySelector(".hover-card")!.textContent).toContain(
      "Server survival — console",
    );
    expect(container.querySelector(".hc-domain")!.textContent).toContain("server/survival");
    // No gutter dismissal contract — the card closes itself when faded.
    expect(container.querySelector(".gutter")).toBeNull();
  });

  it("paints the group card: header, bullet members, the +N More footer", async () => {
    const { container } = await renderCard(HOVER_GROUP_CARD);
    const card = container.querySelector(".hover-card")!;
    expect(card.classList.contains("hc-group")).toBe(true);
    expect(card.textContent).toContain("survival (3 tabs)");
    expect(card.textContent).toContain("•  Server survival");
    expect(card.textContent).toContain("•  Console hub");
    expect(card.textContent).toContain("+ 2 More");
    // A group card has no domain line.
    expect(container.querySelector(".hc-domain")).toBeNull();
  });

  it("slides on popup-update: the new payload's text shows (the old fades on top)", async () => {
    const { container } = await renderCard(HOVER_TAB_CARD);
    await waitFor(() => {
      expect(eventListeners.some((l) => l.event === "shell://popup-update")).toBe(true);
    });
    fireShellEvent("shell://popup-update", { ...HOVER_TAB_CARD, title: "Console hub", domain: "console/hub" });
    await waitFor(() => {
      expect(container.querySelector(".hover-card")!.textContent).toContain("Console hub");
    });
    // The crossfade keeps the previous text on top until it lands.
    expect(container.querySelector(".hc-text-fading")!.textContent).toContain(
      "Server survival — console",
    );
  });

  it("fades on popup-fade and closes its own widget when the fade lands", async () => {
    const { container } = await renderCard(HOVER_TAB_CARD);
    await waitFor(() => {
      expect(eventListeners.some((l) => l.event === "shell://popup-fade")).toBe(true);
    });
    fireShellEvent("shell://popup-fade");
    let card = container.querySelector(".hover-card")!;
    await waitFor(() => {
      card = container.querySelector(".hover-card")!;
      expect(card.classList.contains("hover-card-fading")).toBe(true);
    });
    // The 200ms fade ends (jsdom never runs the CSS — the component
    // hears it through the event the animation would fire; jsdom's
    // AnimationEvent is a stub, so the property rides a raw dispatch).
    const ended = new Event("animationend", { bubbles: true });
    Object.defineProperty(ended, "animationName", { value: "hc-fade-out" });
    fireEvent(card, ended);
    await waitFor(() => {
      expect(invoked.some((call) => call.cmd === "shell_popup_close")).toBe(true);
    });
  });

  it("an update cancels a pending fade (CancelFadeOut)", async () => {
    const { container } = await renderCard(HOVER_TAB_CARD);
    await waitFor(() => {
      expect(eventListeners.some((l) => l.event === "shell://popup-update")).toBe(true);
    });
    fireShellEvent("shell://popup-fade");
    await waitFor(() => {
      expect(container.querySelector(".hover-card")!.classList.contains("hover-card-fading")).toBe(
        true,
      );
    });
    fireShellEvent("shell://popup-update", { ...HOVER_TAB_CARD, title: "Console hub" });
    await waitFor(() => {
      expect(container.querySelector(".hover-card")!.classList.contains("hover-card-fading")).toBe(
        false,
      );
    });
  });

  it("the demo lane renders both review postures from the query", async () => {
    history.replaceState(null, "", "/popup.html?demo&kind=hover-card-group");
    const { container } = render(<PopupApp />);
    await waitFor(() => {
      expect(container.querySelector(".hover-card.hc-group")).not.toBeNull();
    });
    expect(container.querySelector(".hover-card")!.textContent).toContain("survival (4 tabs)");
    expect(container.querySelector(".hover-card")!.textContent).toContain("+ 1 More");
    expect(invoked).toHaveLength(0);
  });
});
