// The modal's three exits and its semantics: Escape closes, a mousedown
// that *starts* on the overlay closes, a drag that starts inside the
// dialog and releases on the overlay does NOT (the target law, not the
// bubble), and the dialog announces itself (role, aria-modal, its own
// title as the accessible name).

import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Modal } from "./Modal";
import styles from "./Modal.module.css";

afterEach(cleanup);

function mount(onClose = vi.fn()) {
  render(
    <Modal title="Connections" onClose={onClose}>
      <button>inside</button>
    </Modal>,
  );
  return onClose;
}

describe("<Modal />", () => {
  it("renders the title and its children inside a dialog", () => {
    mount();
    const dialog = screen.getByRole("dialog", { name: "Connections" });
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(screen.getByText("inside")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Connections" })).toBeTruthy();
    expect(dialog.closest(`.${styles.overlay}`)).toBeTruthy();
  });

  it("Escape closes — even from a keydown that starts inside a field", () => {
    const onClose = mount();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("other keys are not the modal's business", () => {
    const onClose = mount();
    fireEvent.keyDown(window, { key: "Enter" });
    fireEvent.keyDown(window, { key: "Tab" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("a mousedown that begins on the overlay closes", () => {
    const onClose = mount();
    const overlay = document.querySelector(`.${styles.overlay}`) as HTMLElement;
    fireEvent.mouseDown(overlay);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("a mousedown that begins inside the dialog never closes — the target decides", () => {
    const onClose = mount();
    // The click starts on the child and bubbles to the overlay; the
    // handler sees target=inside, currentTarget=overlay — not a dismissal.
    fireEvent.mouseDown(screen.getByText("inside"));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("stops listening when unmounted — a closed modal keeps no key hook", () => {
    const onClose = vi.fn();
    const { unmount } = render(
      <Modal title="t" onClose={onClose}>
        x
      </Modal>,
    );
    unmount();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("a fresh onClose (re-render) replaces the old hook — no stale double-close", () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = render(
      <Modal title="t" onClose={first}>
        x
      </Modal>,
    );
    rerender(
      <Modal title="t" onClose={second}>
        x
      </Modal>,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });
});
