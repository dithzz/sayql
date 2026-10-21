import type { ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { cx } from "../../lib/cx";
import styles from "./TreeRow.module.css";

export interface TreeRowProps {
  depth: number;
  label: ReactNode;
  icon?: ReactNode;
  /** Undefined for leaf rows (no caret). */
  expanded?: boolean;
  selected?: boolean;
  /** Right-aligned secondary text, e.g. a row count or type. */
  meta?: ReactNode;
  /** Shown on hover. */
  actions?: ReactNode;
  title?: string;
  onClick?: () => void;
  onDoubleClick?: () => void;
}

/** One row of a navigator tree. Indentation and caret come from `depth` and `expanded`. */
export function TreeRow({ depth, label, icon, expanded, selected, meta, actions, title, onClick, onDoubleClick }: TreeRowProps) {
  return (
    <div
      className={cx(styles.row, selected && styles.selected)}
      style={{ paddingLeft: 4 + depth * 14 }}
      role="treeitem"
      aria-expanded={expanded}
      aria-selected={selected}
      title={title}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
    >
      <span className={styles.caret}>
        {expanded === undefined ? null : expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
      </span>
      {icon && <span className={styles.icon}>{icon}</span>}
      <span className={styles.label}>{label}</span>
      {meta != null && <span className={styles.meta}>{meta}</span>}
      {actions && (
        <span className={styles.actions} onClick={(e) => e.stopPropagation()}>
          {actions}
        </span>
      )}
    </div>
  );
}
