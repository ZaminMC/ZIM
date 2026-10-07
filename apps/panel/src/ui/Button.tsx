// Design-system button (STYLE-GUIDE: the design system is src/ui/).
// Variants: default surface, primary (accent), danger, ghost (chrome —
// the browser frame's borderless controls, ADR-0015). `busy` swaps the
// label for a spinner while keeping width stable. Native button
// attributes (aria-label, tabIndex, …) pass through untouched.

import styles from "./Button.module.css";

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "primary" | "danger" | "ghost";
  busy?: boolean;
}

export function Button({
  children,
  onClick,
  variant = "default",
  disabled = false,
  busy = false,
  type = "button",
  title,
  className,
  "aria-label": ariaLabel,
  ...rest
}: ButtonProps) {
  const variantClass =
    variant === "primary"
      ? styles.primary
      : variant === "danger"
        ? styles.danger
        : variant === "ghost"
          ? styles.ghost
          : "";
  const classes = [styles.button, variantClass, className, busy ? styles.busy : ""]
    .filter(Boolean)
    .join(" ");
  return (
    <button
      type={type}
      className={classes}
      onClick={onClick}
      disabled={disabled || busy}
      title={title}
      aria-label={ariaLabel}
      {...rest}
    >
      {children}
    </button>
  );
}
