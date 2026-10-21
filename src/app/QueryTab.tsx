import { useEffect, useRef } from "react";
import type { Schema, Settings } from "../api";
import { SplitPane } from "../components/ui";
import { DEMO_EXAMPLES, useSqlGeneration } from "../features/ask";
import { ConfirmWriteDialog, useQueryRunner } from "../features/query";
import { ResultsPanel } from "../features/results";
import type { EditorTab } from "../features/tabs";
import { SqlWorkspace } from "./SqlWorkspace";
import styles from "./QueryTab.module.css";

const DEMO_CONNECTION_ID = "demo";

export interface QueryTabProps {
  tab: EditorTab;
  /** Hidden tabs stay mounted so their SQL, results and running requests survive switching. */
  active: boolean;
  label: string;
  schema: Schema | null;
  settings: Settings;
  modelName: string;
  onChange: (patch: Partial<EditorTab>) => void;
  onOpenSettings: () => void;
}

/** One editor tab: Ask bar and SQL editor above, results below, bound to the tab's database. */
export function QueryTab({ tab, active, label, schema, settings, modelName, onChange, onOpenSettings }: QueryTabProps) {
  const askRef = useRef<HTMLTextAreaElement>(null);
  const setSql = (sql: string) => onChange({ sql });

  const query = useQueryRunner(tab.target);
  const generation = useSqlGeneration({
    target: tab.target,
    slot: tab.id,
    schema,
    model: settings.model,
    hasApiKey: settings.hasApiKey,
    initialQuestion: tab.question,
    onQuestionChange: (question) => onChange({ question }),
    onSql: setSql,
    onRunRequested: query.run,
  });

  const run = () => {
    if (!generation.requestRun()) query.run(tab.sql);
  };

  // Table previews run as soon as the tab is first shown.
  useEffect(() => {
    if (active && tab.autoRun && schema) {
      onChange({ autoRun: false });
      query.run(tab.sql);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, tab.autoRun, schema]);

  useEffect(() => {
    if (active && schema && !tab.sql) askRef.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, schema]);

  return (
    <div hidden={!active} className={styles.tab}>
      <SplitPane
        direction="vertical"
        persistKey="editor"
        initialSize={340}
        minSize={200}
        maxSize={900}
        first={
          <SqlWorkspace
            connectionName={label}
            schema={schema}
            active={active}
            askRef={askRef}
            question={generation.question}
            onQuestionChange={generation.setQuestion}
            status={generation.status}
            modelName={modelName}
            examples={tab.target.connectionId === DEMO_CONNECTION_ID ? DEMO_EXAMPLES : undefined}
            sql={tab.sql}
            onSqlChange={setSql}
            onRun={run}
            running={query.running}
            onOpenSettings={onOpenSettings}
            onRetry={generation.retry}
          />
        }
        second={
          <ResultsPanel
            result={query.result}
            error={query.error}
            running={query.running}
            errorHint={generation.question ? "Try rephrasing your question, or edit the SQL above." : undefined}
          />
        }
      />
      {active && query.pendingWrite && (
        <ConfirmWriteDialog
          sql={query.pendingWrite}
          connectionName={label}
          onCancel={query.cancelWrite}
          onConfirm={query.confirmWrite}
        />
      )}
    </div>
  );
}
