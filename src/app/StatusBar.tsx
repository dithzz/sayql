import { ArrowDownToLine, Circle, MessageSquare, Sparkles } from "lucide-react";
import type { ConnectionConfig, Schema } from "../api";
import type { ModelNames } from "../features/settings";
import type { UpdateState } from "../features/updates";
import { Button } from "../components/ui";
import { cx } from "../lib/cx";
import styles from "./Workbench.module.css";

export interface StatusBarProps {
  active: ConnectionConfig | null;
  databaseName: string | null;
  schema: Schema | null;
  loading: boolean;
  modelNames: ModelNames;
  aiReady: boolean;
  updates: UpdateState;
  onFeedback: () => void;
}

export function StatusBar({ active, databaseName, schema, loading, modelNames, aiReady, updates, onFeedback }: StatusBarProps) {
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
      <Button size="sm" variant="ghost" icon={MessageSquare} onClick={onFeedback} title="Send feedback">
        Feedback
      </Button>
      {updates.update && (
        <Button
          size="sm"
          variant="primary"
          icon={ArrowDownToLine}
          loading={updates.installing}
          title={updates.error ?? updates.update.notes ?? undefined}
          onClick={updates.install}
        >
          {updates.error ? "Update failed — retry" : `Update to ${updates.update.version}`}
        </Button>
      )}
      <span className={styles.statusItem}>
        <Sparkles size={11} />
        {aiReady ? `${modelNames.typing} while typing · ${modelNames.expert} for fixes` : "AI off — set up an AI provider in Settings"}
      </span>
    </footer>
  );
}
