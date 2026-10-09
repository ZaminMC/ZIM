// The application's own question dialogs (P0.3's law: application UI is
// never a window.prompt / window.confirm — those render the webview's
// native gray box, "tauri.localhost says: …", which no theme owns and no
// keyboard contract serves). Both dialogs ride the shared Modal surface,
// so the dark theme, typography, and Escape law come from one place:
//   • ConfirmDialog — a destructive/action question with a sentence, the
//     confirm verb carries the action's own word ("Restart", "Delete"),
//     Enter confirms, Escape cancels.
//   • PromptDialog — one value question with a prefilled, preselected
//     input, a validation sentence under the field, Enter = confirm,
//     Escape = cancel. Validation is the caller's (each verb knows its
//     own honest refusal).

import { useEffect, useRef, useState } from "react";

import { Modal } from "./Modal";
import { Button } from "./Button";
import styles from "./PromptDialog.module.css";

interface ConfirmDialogProps {
  title: string;
  body: string;
  /** The action's own verb on the confirm button ("Delete", "Restart"). */
  confirmLabel: string;
  /** Danger-tinted confirm for destructive verbs. */
  danger?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}

export function ConfirmDialog({
  title,
  body,
  confirmLabel,
  danger = false,
  onConfirm,
  onClose,
}: ConfirmDialogProps) {
  return (
    <Modal title={title} onClose={onClose}>
      <p className={styles.body}>{body}</p>
      <div className={styles.actions}>
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant={danger ? "danger" : "primary"}
          autoFocus
          onClick={() => {
            onConfirm();
            onClose();
          }}
        >
          {confirmLabel}
        </Button>
      </div>
    </Modal>
  );
}

interface PromptDialogProps {
  title: string;
  /** The line under the title that says what the value is ("Server-root path"). */
  hint?: string;
  /** The field's own accessible name — distinct from the dialog's title
   *  so assistive tech (and the tests) can tell question from answer. */
  label: string;
  initial: string;
  confirmLabel: string;
  /** The verb's own refusal — null accepts, a sentence declines. */
  validate?: (value: string) => string | null;
  onConfirm: (value: string) => void;
  onClose: () => void;
}

export function PromptDialog({
  title,
  hint,
  label,
  initial,
  confirmLabel,
  validate,
  onConfirm,
  onClose,
}: PromptDialogProps) {
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [value, setValue] = useState(initial);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const submit = () => {
    const refusal = validate ? validate(value) : null;
    if (refusal) {
      setError(refusal);
      return;
    }
    onConfirm(value);
    onClose();
  };

  return (
    <Modal title={title} onClose={onClose}>
      <form
        className={styles.form}
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        {hint ? <p className={styles.hint}>{hint}</p> : null}
        <input
          ref={inputRef}
          className={styles.input}
          value={value}
          aria-invalid={error ? true : undefined}
          aria-label={label}
          onChange={(event) => {
            setValue(event.target.value);
            if (error) setError(null);
          }}
        />
        {error ? (
          <p className={styles.error} role="alert">
            {error}
          </p>
        ) : null}
        <div className={styles.actions}>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" type="submit">
            {confirmLabel}
          </Button>
        </div>
      </form>
    </Modal>
  );
}
