// The design-system button's contract (STYLE-GUIDE: src/ui IS the design
// system). Pinned: the variant classes, busy's double law (the spinner
// label swaps in AND the button disables), the type="button" default that
// keeps forms from submitting on a stray click, and the pass-through of
// native attributes — a styled wrapper must never swallow aria-label.

import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Button } from "./Button";
import styles from "./Button.module.css";

afterEach(cleanup);

describe("<Button />", () => {
  it("renders its children as a button of type 'button' by default", () => {
    render(<Button>Save</Button>);
    const el = screen.getByRole("button", { name: "Save" });
    expect(el.tagName).toBe("BUTTON");
    expect(el.getAttribute("type")).toBe("button");
  });

  it("carries the base class and adds the variant's own", () => {
    const { rerender } = render(<Button>go</Button>);
    const base = screen.getByRole("button");
    expect(base.className).toContain(styles.button);
    expect(base.className).not.toContain(styles.primary);
    expect(base.className).not.toContain(styles.danger);
    expect(base.className).not.toContain(styles.ghost);

    rerender(<Button variant="primary">go</Button>);
    expect(screen.getByRole("button").className).toContain(styles.primary);
    rerender(<Button variant="danger">go</Button>);
    expect(screen.getByRole("button").className).toContain(styles.danger);
    rerender(<Button variant="ghost">go</Button>);
    expect(screen.getByRole("button").className).toContain(styles.ghost);
  });

  it("busy marks the button, disables the click, and keeps the label in the DOM — the spinner is CSS, the width is stable", () => {
    const onClick = vi.fn();
    render(
      <Button busy onClick={onClick}>
        Save
      </Button>,
    );
    const el = screen.getByRole<HTMLButtonElement>("button");
    expect(el.className).toContain(styles.busy);
    expect(el.textContent).toBe("Save"); // the CSS paints it transparent
    expect(el.disabled).toBe(true);
    fireEvent.click(el);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("a plain disabled button still shows its label and refuses clicks", () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick}>
        Save
      </Button>,
    );
    expect(screen.getByRole("button").textContent).toBe("Save");
    fireEvent.click(screen.getByRole("button"));
    expect(onClick).not.toHaveBeenCalled();
  });
  // (the disabled flag itself is pinned in the pass-through test below)

  it("fires onClick on an enabled click", () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>go</Button>);
    fireEvent.click(screen.getByRole("button"));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("passes native attributes through untouched", () => {
    render(
      <Button aria-label="Stop the server" title="Stop" disabled data-test="x">
        icon
      </Button>,
    );
    const el = screen.getByRole<HTMLButtonElement>("button", { name: "Stop the server" });
    expect(el.getAttribute("title")).toBe("Stop");
    expect(el.getAttribute("data-test")).toBe("x");
    expect(el.disabled).toBe(true);
  });

  it("keeps an explicit type (submit) and merges an extra className", () => {
    render(
      <Button type="submit" className="extra">
        send
      </Button>,
    );
    const el = screen.getByRole("button");
    expect(el.getAttribute("type")).toBe("submit");
    expect(el.className).toContain("extra");
  });
});
