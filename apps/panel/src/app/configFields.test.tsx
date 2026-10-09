// The configuration rows' shared vocabulary (§37–39, ADR-0019,
// ADR-0007): a field always carries its provenance word and never more
// than one clear affordance (and only for an actual override), a static
// fact shows its value without pretending to be editable, and §82's
// reserved room is stated as reserved — the word on the row, never a
// fake control.

import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FieldRow, ReservedRow, Rows, StaticRow } from "./configFields";

afterEach(cleanup);

describe("<FieldRow />", () => {
  it("pairs the label with the control and states the provenance word", () => {
    render(
      <FieldRow label="Max players" provenance="global">
        <input aria-label="Max players" />
      </FieldRow>,
    );
    expect(screen.getByText("Max players")).toBeTruthy();
    expect(screen.getByText("global")).toBeTruthy();
    expect(screen.getByText("global").getAttribute("title")).toBe(
      "Inherited from the global defaults",
    );
  });

  it("the custom provenance names itself as this server's own value", () => {
    render(
      <FieldRow label="Max players" provenance="custom">
        <input />
      </FieldRow>,
    );
    const word = screen.getByText("custom");
    expect(word.className).not.toBe(""); // styled apart from the inherited word
    expect(word.getAttribute("title")).toBe("This server sets its own value");
  });

  it("the clear affordance exists only for an override, and is the label's own", () => {
    const onClear = vi.fn();
    const { rerender } = render(
      <FieldRow label="View distance" provenance="custom" onClear={onClear}>
        <input />
      </FieldRow>,
    );
    const clear = screen.getByRole("button", { name: "Clear the View distance override" });
    fireEvent.click(clear);
    expect(onClear).toHaveBeenCalledTimes(1);

    rerender(
      <FieldRow label="View distance" provenance="global">
        <input />
      </FieldRow>,
    );
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("a per-server-only field (no provenance) shows no provenance word", () => {
    render(
      <FieldRow label="Whitelist">
        <input />
      </FieldRow>,
    );
    expect(screen.queryByText("global")).toBeNull();
    expect(screen.queryByText("custom")).toBeNull();
  });

  it("the hint renders when given and stays absent when not", () => {
    const { rerender } = render(
      <FieldRow label="Motd" hint="Shown in the server list">
        <input />
      </FieldRow>,
    );
    expect(screen.getByText("Shown in the server list")).toBeTruthy();
    rerender(
      <FieldRow label="Motd">
        <input />
      </FieldRow>,
    );
    expect(screen.queryByText("Shown in the server list")).toBeNull();
  });
});

describe("<StaticRow />", () => {
  it("shows the value as fact, with no editable control and no clear", () => {
    render(<StaticRow label="Server ID" value="survival" />);
    expect(screen.getByText("survival")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("carries its hint", () => {
    render(<StaticRow label="Server ID" value="survival" hint="Chosen at registration" />);
    expect(screen.getByText("Chosen at registration")).toBeTruthy();
  });
});

describe("<ReservedRow />", () => {
  it("states the reserved room with the word and the note — never a control", () => {
    render(<ReservedRow label="Backups" note="A future release manages snapshots here." />);
    expect(screen.getByText("reserved")).toBeTruthy();
    expect(screen.getByText("A future release manages snapshots here.")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
  });
});

describe("<Rows />", () => {
  it("groups the rows it is given", () => {
    render(
      <Rows>
        <StaticRow label="a" value="1" />
        <StaticRow label="b" value="2" />
      </Rows>,
    );
    expect(screen.getByText("a")).toBeTruthy();
    expect(screen.getByText("b")).toBeTruthy();
  });
});
