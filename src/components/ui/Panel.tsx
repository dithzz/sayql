import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import { cx } from "../../lib/cx";
import styles from "./Panel.module.css";

export interface PanelProps {
  title: string;
  icon?: LucideIcon;
  subtitle?: ReactNode;
  /** Controls on the right of the header. */
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}

/** A titled workbench region (navigator, editor, results), like a DBeaver view. */
export function Panel({ title, icon: Icon, subtitle, actions, children, className }: PanelProps) {
  return (
    <section className={cx(styles.panel, className)} aria-label={title}>
      <header className={styles.header}>
        {Icon && (
          <span className={styles.icon}>
            <Icon size={14} />
          </span>
        )}
        <span className={styles.title}>{title}</span>
        {subtitle && <span className={styles.subtitle}>{subtitle}</span>}
        {actions && <div className={styles.actions}>{actions}</div>}
      </header>
      <div className={styles.body}>{children}</div>
    </section>
  );
}
