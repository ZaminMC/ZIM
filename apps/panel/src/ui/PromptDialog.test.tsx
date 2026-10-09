// The application-owned dialog law's tests (P0.3): the question UI that
// replaced the native gray box (`tauri.localhost says: …`) behaves like a
// first-class surface — the confirm verb carries the action's own word,
// validation is the verb's own honest refusal, a refusal keeps the dialog
// and the value, and typing clears the stale refusal.

import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConfirmDialog, PromptDialog } from "./PromptDialog";

afterEach(cleanup);

describe("ConfirmDialog", () => {
  it("asks the question with the action's own verb", () => {
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    render(
      <ConfirmDialog
        title="Delete the backup?"
        body="Backup 3 will be gone for good."
        confirmLabel="Delete"
        danger
        onConfirm={onConfirm}
        onClose={onClose}
      />,
    );
    expect(screen.getByText("Delete the backup?")).toBeTruthy();
    expect(screen.getByText("Backup 3 will be gone for good.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Delete" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
  });

  it("confirm runs the action and closes — once, in that order", () => {
    const calls: string[] = [];
    const onConfirm = vi.fn(() => calls.push("confirm"));
    const onClose = vi.fn(() => calls.push("close"));
    render(
      <ConfirmDialog title="t" body="b" confirmLabel="Restart" onConfirm={onConfirm} onClose={onClose} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));
    expect(calls).toEqual(["confirm", "close"]);
  });

  it("cancel closes without running the action", () => {
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    render(
      <ConfirmDialog title="t" body="b" confirmLabel="Delete" onConfirm={onConfirm} onClose={onClose} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });
});

describe("PromptDialog", () => {
  it("prefills the value, names the field for assistive tech, autofocuses it", () => {
    render(
      <PromptDialog
        title="Rename the server"
        label="Server name"
        initial="survival"
        confirmLabel="Rename"
        onConfirm={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const input = screen.getByRole<HTMLInputElement>("textbox", { name: "Server name" });
    expect(input.value).toBe("survival");
    expect(document.activeElement).toBe(input);
  });

  it("shows the hint that says what the value is", () => {
    render(
      <PromptDialog
        title="Add a server"
        hint="Server-root path"
        label="Path"
        initial=""
        confirmLabel="Add"
        onConfirm={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("Server-root path")).toBeTruthy();
  });

  it("submitting (Enter on the form) confirms with the current value and closes", () => {
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    render(
      <PromptDialog
        title="Rename the server"
        label="Server name"
        initial="survival"
        confirmLabel="Rename"
        onConfirm={onConfirm}
        onClose={onClose}
      />,
    );
    const input = screen.getByRole("textbox", { name: "Server name" });
    fireEvent.change(input, { target: { value: "creative" } });
    fireEvent.submit(input.closest("form")!);
    expect(onConfirm).toHaveBeenCalledWith("creative");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("a validation refusal keeps the dialog, the value, and says why", () => {
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    render(
      <PromptDialog
        title="New group"
        label="Group name"
        initial=""
        confirmLabel="Create group"
        validate={(value) => (value.trim() === "" ? "Give the group a name." : null)}
        onConfirm={onConfirm}
        onClose={onClose}
      />,
    );
    const input = screen.getByRole("textbox", { name: "Group name" });
    fireEvent.submit(input.closest("form")!);
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toBe("Give the group a name.");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(onConfirm).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("typing clears the stale refusal", () => {
    render(
      <PromptDialog
        title="New group"
        label="Group name"
        initial=""
        confirmLabel="Create group"
        validate={(value) => (value.trim() === "" ? "Give the group a name." : null)}
        onConfirm={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const input = screen.getByRole("textbox", { name: "Group name" });
    fireEvent.submit(input.closest("form")!);
    expect(screen.getByRole("alert")).toBeTruthy();
    fireEvent.change(input, { target: { value: "s" } });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("a valid resubmit after a refusal confirms", () => {
    const onConfirm = vi.fn();
    render(
      <PromptDialog
        title="New group"
        label="Group name"
        initial=""
        confirmLabel="Create group"
        validate={(value) => (value.trim() === "" ? "Give the group a name." : null)}
        onConfirm={onConfirm}
        onClose={vi.fn()}
      />,
    );
    const input = screen.getByRole("textbox", { name: "Group name" });
    fireEvent.submit(input.closest("form")!);
    fireEvent.change(input, { target: { value: "builders" } });
    fireEvent.submit(input.closest("form")!);
    expect(onConfirm).toHaveBeenCalledWith("builders");
  });

  it("cancel closes without confirming", () => {
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    render(
      <PromptDialog
        title="t"
        label="l"
        initial="x"
        confirmLabel="OK"
        onConfirm={onConfirm}
        onClose={onClose}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
