import { Circle, Sparkles } from "lucide-react";
import type { ConnectionConfig, Schema } from "../api";
import type { ModelNames } from "../features/settings";
import { cx } from "../lib/cx";
import styles from "./Workbench.module.css";

export interface StatusBarProps {
  active: ConnectionConfig | null;
  databaseName: string | null;
  schema: Schema | null;
  loading: boolean;
  modelNames: ModelNames;
  hasApiKey: boolean;
}

export function StatusBar({ active, databaseName, schema, loading, modelNames, hasApiKey }: StatusBarProps) {
  const columns = schema?.tables.reduce((n, t) => n + t.columns.length, 0) ?? 0;
  const state = !active ? "Not connected" : loading ? "Connecting…" : schema ? "Connected" : "Disconnected";
  return (
    <footer className={styles.statusbar}>
      <span className={styles.statusItem}>
        <Circle size={8} className={cx(styles.dot, schema && !loading && styles.dotOk)} fill="currentColor" />
        {state}
        {active && ` · ${active.name}`}
        {active && databaseName && active.kind !== "sqlite" && ` / ${databaseName}`}
      </span>
      {schema && (
        <span className={styles.statusItem}>
          {schema.tables.length} tables · {columns} columns
        </span>
      )}
      <span className={styles.spacer} />
      <span className={styles.statusItem}>
        <Sparkles size={11} />
        {hasApiKey ? `${modelNames.typing} while typing · ${modelNames.expert} for fixes` : "AI off — add an API key in Settings"}
      </span>
    </footer>
  );
}
