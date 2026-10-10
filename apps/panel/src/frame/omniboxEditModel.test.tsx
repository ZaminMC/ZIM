// The omnibox EDIT model's view laws (ADR-0032 lane 2, backported from
// components/omnibox/browser/omnibox_edit_model.cc + omnibox_view_views.cc):
//
//   Escape  — OnEscapeKeyPressed's two stages: restore the pre-edit text
//             (focus STAYS, all selected), then a bare Esc leaves.
//   Alt+Enter — OpenURL's kNEW_FOREGROUND_TAB disposition; plain Enter
//             commits into the tab the hand is in.
//   Paste / Paste and go — OmniboxViewViews::ShowContextMenu's verbs:
//             paste edits the field (the operator owns the commit),
//             paste-and-go commits the clipboard text directly.
//   The note — the inline classification announced BEFORE the commit.

import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { FrameApp } from "./FrameApp";
import type * as FrameIpc from "./frameIpc";

// The frame speaks to the host through frameIpc; the view tests record
// the commits and script the classifier, everything else stays real.
const commits: Array<{ text: string; newTab: boolean }> = [];
let clip = "";
let classifyFor: (text: string) => unknown = () => null;

vi.mock("./frameIpc", async (importOriginal) => {
  const actual = await importOriginal<typeof FrameIpc>();
  return {
    ...actual,
    omniboxCommit: (text: string, newTab?: boolean) => {
      commits.push({ text, newTab: newTab ?? false });
      return Promise.resolve({ kind: "query", text });
    },
    omniboxClassify: (text: string) =>
      Promise.resolve(classifyFor(text) ?? null),
    clipboardText: () => Promise.resolve(clip),
  };
});

describe("<FrameApp /> the omnibox edit model", () => {
  beforeEach(() => {
    history.replaceState(null, "", "/frame.html?demo");
    commits.length = 0;
    clip = "";
    classifyFor = () => null;
  });
  afterEach(() => {
    cleanup();
  });

  async function bootOmnibox() {
    const view = render(<FrameApp />);
    await waitFor(() => {
      expect(view.container.querySelector(".omnibox")).toBeTruthy();
    });
    // The boot's emit storm settles first — a snapshot landing mid-test
    // would reset the field (the rest law), and the laws under test are
    // the edit model's, not the boot's.
    await new Promise((resolve) => setTimeout(resolve, 150));
    const input = view.container.querySelector<HTMLInputElement>(".omnibox")!;
    return { view, input, resting: input.value };
  }

  const pasteRows = () =>
    Array.from(
      document.querySelectorAll<HTMLButtonElement>(".context button"),
    ).filter(
      (b) => b.textContent === "Paste" || b.textContent === "Paste and go",
    );

  it("the FIRST Escape restores the pre-edit text and keeps the focus, all selected", async () => {
    const { input, resting } = await bootOmnibox();
    input.focus();
    fireEvent.change(input, { target: { value: "paper" } });
    expect(input.value).toBe("paper");
    fireEvent.keyDown(input, { key: "Escape" });
    // The display reverts to the permanent text…
    expect(input.value).toBe(resting);
    // …the field KEEPS the focus (blur is NOT the law)…
    expect(document.activeElement).toBe(input);
    // …and the restored text sits selected, ready to be retyped over.
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe(resting.length);
    // Nothing was committed.
    expect(commits).toEqual([]);
  });

  it("a SECOND Escape, nothing left to revert, leaves the field", async () => {
    const { input } = await bootOmnibox();
    input.focus();
    fireEvent.change(input, { target: { value: "paper" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(document.activeElement).toBe(input);
    fireEvent.keyDown(input, { key: "Escape" });
    expect(document.activeElement).not.toBe(input);
    expect(commits).toEqual([]);
  });

  it("an Escape on an untouched field just leaves it", async () => {
    const { input } = await bootOmnibox();
    input.focus();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(document.activeElement).not.toBe(input);
    expect(commits).toEqual([]);
  });

  it("plain Enter commits into the tab the hand is in (kCurrentTab)", async () => {
    const { input } = await bootOmnibox();
    input.focus();
    fireEvent.change(input, { target: { value: "zim://settings/" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => {
      expect(commits).toEqual([
        { text: "zim://settings/", newTab: false },
      ]);
    });
  });

  it("Alt+Enter opens the request in a NEW FOREGROUND tab (OpenURL's law)", async () => {
    const { input } = await bootOmnibox();
    input.focus();
    fireEvent.change(input, { target: { value: "zim://settings/" } });
    fireEvent.keyDown(input, { key: "Enter", altKey: true });
    await waitFor(() => {
      expect(commits).toEqual([{ text: "zim://settings/", newTab: true }]);
    });
  });

  it("the field's own context menu carries Paste and Paste and go", async () => {
    const { input } = await bootOmnibox();
    clip = "localhost:25565";
    fireEvent.contextMenu(input);
    await waitFor(() => {
      expect(pasteRows()).toHaveLength(2);
    });
    const rows = pasteRows();
    expect(rows.map((b) => b.textContent)).toEqual(["Paste", "Paste and go"]);
    // The clipboard had text at open time — neither row is disabled.
    expect(rows.every((b) => !b.disabled)).toBe(true);
    // Paste and go commits the clipboard text directly (kCurrentTab) —
    // the field never edits.
    fireEvent.click(rows.find((b) => b.textContent === "Paste and go")!);
    await waitFor(() => {
      expect(commits).toEqual([{ text: "localhost:25565", newTab: false }]);
    });
  });

  it("Paste puts the clipboard text into the field as an edit", async () => {
    const { input } = await bootOmnibox();
    clip = "zim://servers/";
    fireEvent.contextMenu(input);
    await waitFor(() => {
      expect(pasteRows()).toHaveLength(2);
    });
    fireEvent.click(pasteRows().find((b) => b.textContent === "Paste")!);
    await waitFor(() => {
      expect(input.value).toBe("zim://servers/");
    });
    // The operator still owns the commit — nothing was sent.
    expect(commits).toEqual([]);
  });

  it("an empty clipboard disables the paste rows honestly", async () => {
    const { input } = await bootOmnibox();
    clip = "";
    fireEvent.contextMenu(input);
    await waitFor(() => {
      expect(pasteRows()).toHaveLength(2);
    });
    expect(pasteRows().every((b) => b.disabled)).toBe(true);
  });

  it("the classification is announced before the commit", async () => {
    const { input } = await bootOmnibox();
    const { container } = { container: document.body };
    classifyFor = (text) => {
      if (text === "localhost:25565")
        return { Join: { host: "localhost", port: 25565 } };
      if (text === "zim://settings/") return { Internal: { kind: "settings" } };
      if (text === "paper") return { Query: "paper" };
      if (text === "zim://nonsense/")
        return { Internal: { kind: "missing" } };
      return null;
    };
    input.focus();
    fireEvent.change(input, { target: { value: "localhost:25565" } });
    await waitFor(() => {
      expect(
        container.querySelector(".omnibox-note")?.textContent,
      ).toContain("localhost:25565");
    });
    fireEvent.change(input, { target: { value: "zim://settings/" } });
    await waitFor(() => {
      expect(container.querySelector(".omnibox-note")?.textContent).toBe(
        "ZIM page · settings",
      );
    });
    fireEvent.change(input, { target: { value: "paper" } });
    await waitFor(() => {
      expect(container.querySelector(".omnibox-note")?.textContent).toBe(
        "search",
      );
    });
    fireEvent.change(input, { target: { value: "zim://nonsense/" } });
    await waitFor(() => {
      expect(container.querySelector(".omnibox-note")?.textContent).toBe(
        "no ZIM page",
      );
    });
  });
});
