// Shared row primitives for the three configuration surfaces (§37–39,
// ADR-0019). FieldRow pairs a labeled control with its provenance word —
// ADR-0007's whole point was that the UI never guesses where a value came
// from — and an explicit clear affordance when the field carries this
// server's own override. ReservedRow is §82's honesty rule made visible:
// a room the model does not have yet is stated as reserved, never faked.

import type { ReactNode } from "react";
import type { FieldProvenance } from "../protocol/types";
import styles from "./configFields.module.css";

export function FieldRow({
  label,
  provenance,
  onClear,
  hint,
  children,
}: {
  label: string;
  /** Absent for per-server-only fields that have no global default. */
  provenance?: FieldProvenance;
  /** Present only when the field carries this server's own override. */
  onClear?: () => void;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className={styles.row}>
      <span className={styles.label}>{label}</span>
      <span className={styles.control}>{children}</span>
      <span className={styles.rowActions}>
        {provenance ? (
          <span
            className={`${styles.prov} ${provenance === "custom" ? styles.provCustom : ""}`}
            title={
              provenance === "custom"
                ? "This server sets its own value"
                : "Inherited from the global defaults"
            }
          >
            {provenance}
          </span>
        ) : null}
        {onClear ? (
          <button
            type="button"
            className={styles.clear}
            onClick={onClear}
            aria-label={`Clear the ${label} override`}
            title="Drop this override — the global default applies again"
          >
            ×
          </button>
        ) : null}
      </span>
      {hint ? <span className={styles.hint}>{hint}</span> : null}
    </div>
  );
}

export function StaticRow({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className={styles.row}>
      <span className={styles.label}>{label}</span>
      <span className={styles.control}>
        <span className={styles.staticValue}>{value}</span>
      </span>
      <span />
      {hint ? <span className={styles.hint}>{hint}</span> : null}
    </div>
  );
}

export function ReservedRow({ label, note }: { label: string; note: string }) {
  return (
    <div className={`${styles.row} ${styles.rowReserved}`}>
      <span className={styles.label}>{label}</span>
      <span className={styles.reservedTag}>reserved</span>
      <span className={styles.hint}>{note}</span>
    </div>
  );
}

export function Rows({ children }: { children: ReactNode }) {
  return <div className={styles.rows}>{children}</div>;
}
