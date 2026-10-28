import { useEffect, type ReactNode } from "react";
import { X } from "lucide-react";
import { cx } from "../../lib/cx";
import { IconButton } from "./Button";
import styles from "./Dialog.module.css";

export interface DialogProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Buttons on the right of the footer. */
  actions?: ReactNode;
  /** Content on the left of the footer (e.g. a destructive action). */
  secondaryActions?: ReactNode;
  size?: "sm" | "md" | "lg";
}

/** Modal dialog. Closes on Escape or a click on the backdrop. */
export function Dialog({ title, onClose, children, actions, secondaryActions, size = "md" }: DialogProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className={styles.backdrop} onMouseDown={onClose}>
      <div
        className={cx(styles.dialog, styles[size])}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className={styles.header}>
          <h2 className={styles.title}>{title}</h2>
          <IconButton icon={X} label="Close" size="sm" onClick={onClose} />
        </header>
        <div className={styles.body}>{children}</div>
        {(actions || secondaryActions) && (
          <footer className={styles.footer}>
            {secondaryActions}
            <span className={styles.spacer} />
            {actions}
          </footer>
        )}
      </div>
    </div>
  );
}
