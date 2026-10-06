// Modal dialog: overlay + Esc to close. Focus management stays light for
// this slice; the dialog is the first focusable element via autoFocus use.

import { useEffect } from "react";
import styles from "./Modal.module.css";

export interface ModalProps {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
}

export function Modal({ title, onClose, children }: ModalProps) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className={styles.overlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className={styles.dialog} role="dialog" aria-modal="true" aria-label={title}>
        <h2 className={styles.title}>{title}</h2>
        {children}
      </div>
    </div>
  );
}
