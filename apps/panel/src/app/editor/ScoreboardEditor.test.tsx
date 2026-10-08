// The scoreboard editor (§34): configuration left, live preview right.
// The regression surface is the founder's promise: keystrokes land in
// the row model (the draft a Save sends), the spec re-derives from
// those rows, and the preview is a pure view of them — the same live
// loop the Files view runs.

import { useState } from "react";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ScoreboardEditor } from "./ScoreboardEditor";
import { parseProperties, serializeProperties, type PropertyLine } from "./properties";
import { readScoreboard, writeScoreboardTitle, writeScoreboardRows } from "./scoreboard";

function setup(text: string) {
  // The harness mirrors FilesView: the spec re-derives from the row
  // model after every edit, so the preview never sees a stale spec.
  let current = parseProperties(text);
  function Harness() {
    const [lines, setLines] = useState(current);
    const apply = (next: PropertyLine[]) => {
      current = next;
      setLines(next);
    };
    const spec = readScoreboard(lines)!;
    return (
      <ScoreboardEditor
        spec={spec}
        onTitle={(title) => apply(writeScoreboardTitle(lines, title))}
        onRows={(rows) => apply(writeScoreboardRows(lines, spec, rows))}
      />
    );
  }
  render(<Harness />);
  return () => serializeProperties(current);
}

describe("ScoreboardEditor", () => {
  afterEach(cleanup);

  it("renders the two panes: configuration and the live preview", () => {
    setup("title=&c&lSurvival\nline.1=Players: 12\nline.2=Money: $100\n");
    expect(screen.getByLabelText("Scoreboard editor")).toBeTruthy();
    expect(screen.getByLabelText("Live scoreboard preview")).toBeTruthy();
    // The preview shows the title without the codes, styled red+bold.
    expect(screen.getByText("Survival")).toBeTruthy();
  });

  it("a title keystroke rewrites the title pair through the row model", () => {
    const serialize = setup("title=Old\nline.1=A\n");
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "&aNew" } });
    expect(serialize()).toBe("title=&aNew\nline.1=A\n");
  });

  it("a line keystroke rewrites exactly that row", () => {
    const serialize = setup("title=T\nline.1=Players: 12\nline.2=Money: $100\n");
    fireEvent.change(screen.getByLabelText("Line 1"), {
      target: { value: "Players: 13" },
    });
    expect(serialize()).toBe("title=T\nline.1=Players\\: 13\nline.2=Money\\: $100\n");
  });

  it("add and remove edit the rows structurally", () => {
    const serialize = setup("title=T\nline.1=A\n");
    fireEvent.click(screen.getByRole("button", { name: /Add line/ }));
    expect(serialize()).toBe("title=T\nline.1=A\nline.2=\n");
    fireEvent.click(screen.getByRole("button", { name: "Remove line 1" }));
    expect(serialize()).toBe("title=T\nline.2=\n");
  });

  it("the preview follows the rows as they change", () => {
    setup("title=T\nline.1=Players: 12\n");
    expect(screen.getByText("Players: 12")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Line 1"), {
      target: { value: "Players: 13" },
    });
    expect(screen.getByText("Players: 13")).toBeTruthy();
    expect(screen.queryByText("Players: 12")).toBeNull();
  });

  it("move up/down reorders the rows", () => {
    const serialize = setup("title=T\nline.1=A\nline.2=B\n");
    fireEvent.click(screen.getByRole("button", { name: "Move line 2 up" }));
    expect(serialize()).toBe("title=T\nline.1=B\nline.2=A\n");
  });

  it("move buttons respect the edges", () => {
    setup("title=T\nline.1=A\nline.2=B\n");
    expect(screen.getByRole("button", { name: "Move line 1 up" })).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: "Move line 2 down" })).toHaveProperty(
      "disabled",
      true,
    );
  });
});
