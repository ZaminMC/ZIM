// The extensions page (§56/§57, ADR-0031): the daemon's inventory is
// rendered as it answered — valid manifests with their declared
// permissions (data permissions visually distinguished from
// contribution ones), unreadable folders named with their reasons, an
// empty dir an honest empty state, and the reserved room stated once.

import { render, screen, cleanup, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ExtensionsPage } from "./ExtensionsPage";
import type { ExtensionsListResult } from "../protocol/types";

vi.mock("../state/actions", () => ({
  listExtensions: vi.fn(),
}));

import { listExtensions } from "../state/actions";
const listMock = listExtensions as ReturnType<typeof vi.fn>;

function answer(over: Partial<ExtensionsListResult> = {}): ExtensionsListResult {
  return {
    directory: "/home/you/.local/share/zamind/extensions",
    extensions: [],
    problems: [],
    contributionsActive: false,
    ...over,
  };
}

beforeEach(() => {
  listMock.mockReset().mockResolvedValue(answer());
});

afterEach(cleanup);

describe("ExtensionsPage", () => {
  it("an empty inventory states where extension folders live — not a fake list", async () => {
    render(<ExtensionsPage />);
    await waitFor(() => expect(screen.getByText(/No extensions installed/)).toBeTruthy());
    expect(screen.getByText(/zamin-extension\.toml/)).toBeTruthy();
    expect(screen.getByText("/home/you/.local/share/zamind/extensions")).toBeTruthy();
  });

  it("renders the declarations: permissions as chips, data access apart", async () => {
    listMock.mockResolvedValue(
      answer({
        extensions: [
          {
            id: "export-config",
            name: "Export configuration",
            version: "0.3.0",
            description: "Adds an export action to the server context menu.",
            permissions: ["contribution:context-menu", "data:files.read"],
            directory: "export-config",
          },
        ],
      }),
    );
    render(<ExtensionsPage />);
    await waitFor(() => expect(screen.getByText("Export configuration")).toBeTruthy());
    expect(screen.getByText("v0.3.0")).toBeTruthy();
    expect(screen.getByText("Adds an export action to the server context menu.")).toBeTruthy();
    const chips = screen.getAllByText(/^(contribution|data):/);
    expect(chips).toHaveLength(2);
    // The loud family rides a data attribute the stylesheet separates.
    expect(chips.find((chip) => chip.textContent === "data:files.read")?.dataset.family).toBe(
      "data",
    );
    expect(screen.getByText("Folder: export-config")).toBeTruthy();
  });

  it("unreadable folders are named with their reasons, never skipped", async () => {
    listMock.mockResolvedValue(
      answer({
        problems: [
          {
            directory: "broken",
            reason: "zamin-extension.toml did not parse: unknown variant `data:*`",
          },
        ],
      }),
    );
    render(<ExtensionsPage />);
    await waitFor(() => expect(screen.getByText("broken")).toBeTruthy());
    expect(screen.getByText(/did not parse/)).toBeTruthy();
    expect(screen.getByText(/Folders that could not be read/)).toBeTruthy();
  });

  it("states the reserved room: declarations exist, contributions do not yet", async () => {
    listMock.mockResolvedValue(
      answer({
        extensions: [
          {
            id: "quiet",
            name: "Quiet",
            version: "1.0.0",
            permissions: [],
            directory: "quiet",
          },
        ],
      }),
    );
    render(<ExtensionsPage />);
    await waitFor(() => expect(screen.getByText("Quiet")).toBeTruthy());
    expect(screen.getByText(/nothing contributes yet/)).toBeTruthy();
    expect(
      screen.getByText(/Declares no permissions — nothing will be granted/),
    ).toBeTruthy();
  });

  it("a failed read is a typed error, not a fake empty inventory", async () => {
    listMock.mockRejectedValue(new Error("the daemon is unreachable"));
    render(<ExtensionsPage />);
    await waitFor(() => expect(screen.getByText(/unreachable/)).toBeTruthy());
    expect(screen.queryByText(/No extensions installed/)).toBeNull();
  });
});
