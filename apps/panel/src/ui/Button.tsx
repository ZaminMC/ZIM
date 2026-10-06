// Design-system button (STYLE-GUIDE: the design system is src/ui/).
// Variants: default surface, primary (accent), danger. `busy` swaps the
// label for a spinner while keeping width stable.

import styles from "./Button.module.css";

export interface ButtonProps {
  children: React.ReactNode;
  onClick?: () => void;
  variant?: "default" | "primary" | "danger";
  disabled?: boolean;
  busy?: boolean;
  type?: "button" | "submit";
  title?: string;
}

export function Button({
  children,
  onClick,
  variant = "default",
  disabled = false,
  busy = false,
  type = "button",
  title,
}: ButtonProps) {
  const variantClass =
    variant === "primary" ? styles.primary : variant === "danger" ? styles.danger : "";
  const classes = [styles.button, variantClass, busy ? styles.busy : ""]
    .filter(Boolean)
    .join(" ");
  return (
    <button
      type={type}
      className={classes}
      onClick={onClick}
      disabled={disabled || busy}
      title={title}
    >
      {children}
    </button>
  );
}
