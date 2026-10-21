import { useEffect, useRef } from "react";
import { Wand2 } from "lucide-react";
import type { Schema, Settings } from "../api";
import { Button, SplitPane } from "../components/ui";
import { DEMO_EXAMPLES, useSqlGeneration, type TaskKind } from "../features/ask";
import { ConfirmWriteDialog, useQueryRunner } from "../features/query";
import { ResultsPanel } from "../features/results";
import type { ModelNames } from "../features/settings";
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
  modelNames: ModelNames;
  onChange: (patch: Partial<EditorTab>) => void;
  onOpenSettings: () => void;
  onSave: () => void;
}

/** One editor tab: Ask bar and SQL editor above, results below, bound to the tab's database. */
export function QueryTab({ tab, active, label, schema, settings, modelNames, onChange, onOpenSettings, onSave }: QueryTabProps) {
  const askRef = useRef<HTMLTextAreaElement>(null);
  const setSql = (sql: string) => onChange({ sql });
  const lastRun = tab.lastRun ?? null;
  /** A failed AI query is fixed automatically once; if the fix fails too, the person decides. */
  const autoFixUsed = useRef(false);

  const query = useQueryRunner(tab.target, (sql, error) => {
    if (autoFixUsed.current || !settings.hasApiKey || !generation.question.trim()) return;
    autoFixUsed.current = true;
    generation.fix(sql, error);
  });

  /** Runs `sql` and remembers it as the query follow-ups build on. */
  const runAndRemember = (sql: string, question: string, task: TaskKind | "manual" = "manual") => {
    if (task !== "fix") autoFixUsed.current = false;
    onChange({ lastRun: sql.trim() ? { question, sql } : null });
    query.run(sql);
  };

  const generation = useSqlGeneration({
    target: tab.target,
    slot: tab.id,
    schema,
    typingModel: settings.typingModel,
    hasApiKey: settings.hasApiKey,
    initialQuestion: tab.question,
    previous: lastRun,
    onQuestionChange: (question) => onChange({ question }),
    onSql: setSql,
    onRunRequested: (sql, task) => runAndRemember(sql, generation.question, task),
  });

  const run = () => {
    if (!generation.requestRun()) runAndRemember(tab.sql, generation.question);
  };

  // Table previews run as soon as the tab is first shown.
  useEffect(() => {
    if (active && tab.autoRun && schema) {
      onChange({ autoRun: false });
      runAndRemember(tab.sql, "");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, tab.autoRun, schema]);

  useEffect(() => {
    if (active && schema && !tab.sql) askRef.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, schema]);

  const busy = generation.status.kind === "writing";
  const canUseAi = settings.hasApiKey && !!schema;
  const autoFixFailed = autoFixUsed.current && !busy && !!query.error;

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
            modelNames={modelNames}
            examples={tab.target.connectionId === DEMO_CONNECTION_ID ? DEMO_EXAMPLES : undefined}
            sql={tab.sql}
            onSqlChange={setSql}
            onRun={run}
            running={query.running}
            onImprove={() => generation.improve(tab.sql)}
            canImprove={canUseAi && !busy && !!tab.sql.trim()}
            onSave={onSave}
            onOpenSettings={onOpenSettings}
            onRetry={generation.retry}
            followUp={lastRun ? { onStartFresh: () => onChange({ lastRun: null }) } : undefined}
          />
        }
        second={
          <ResultsPanel
            result={query.result}
            error={query.error}
            running={query.running}
            errorHint={
              autoFixFailed
                ? `${modelNames.expert} couldn't fix it automatically. Try rephrasing your question, or edit the SQL above.`
                : undefined
            }
            errorAction={
              canUseAi && (
                <Button
                  size="sm"
                  variant="ghost"
                  icon={Wand2}
                  disabled={busy}
                  onClick={() => query.error && generation.fix(tab.sql, query.error)}
                >
                  Fix with AI
                </Button>
              )
            }
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
