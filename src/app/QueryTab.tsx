import { useEffect, useRef } from "react";
import { Wand2 } from "lucide-react";
import type { Schema, Settings, Skill } from "../api";
import { Button, SplitPane } from "../components/ui";
import { parseCommand, planRequest, useSqlGeneration, type TaskKind } from "../features/ask";
import { ConfirmWriteDialog, useQueryRunner } from "../features/query";
import { ResultsPanel } from "../features/results";
import type { ModelNames } from "../features/settings";
import type { EditorTab } from "../features/tabs";
import { SqlWorkspace } from "./SqlWorkspace";
import styles from "./QueryTab.module.css";

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
  skills: Skill[];
  /** "Create skill…" in the "/" menu. */
  onCreateSkill: () => void;
  onSave: () => void;
}

/** One editor tab: Ask bar and SQL editor above, results below, bound to the tab's database. */
export function QueryTab({ tab, active, label, schema, settings, modelNames, onChange, onOpenSettings, skills, onCreateSkill, onSave }: QueryTabProps) {
  const askRef = useRef<HTMLTextAreaElement>(null);
  const setSql = (sql: string) => onChange({ sql });
  const lastRun = tab.lastRun ?? null;
  const focusEnabled = tab.focusEnabled ?? true;
  /** A failed AI query is fixed automatically once; if the fix fails too, the person decides. */
  const autoFixUsed = useRef(false);

  const query = useQueryRunner(tab.target, (sql, error) => {
    if (autoFixUsed.current || !settings.aiReady || !generation.question.trim()) return;
    autoFixUsed.current = true;
    generation.fix(sql, error);
  });

  /** Runs `sql` and remembers it as the query follow-ups build on. */
  const runAndRemember = (sql: string, question: string, task: TaskKind | "manual" = "manual") => {
    if (task !== "fix") autoFixUsed.current = false;
    // Follow-ups see the question without its "/command".
    onChange({ lastRun: sql.trim() ? { question: parseCommand(question, skills).rest, sql } : null });
    query.run(sql);
  };

  const generation = useSqlGeneration({
    target: tab.target,
    slot: tab.id,
    schema,
    typingModel: `${settings.provider}:${settings.typingModel}`,
    aiReady: settings.aiReady,
    initialQuestion: tab.question,
    previous: lastRun,
    // Hidden picks stay selected but don't reach the AI.
    focusTables: focusEnabled ? (tab.focusTables ?? []) : [],
    skills,
    sql: tab.sql,
    onQuestionChange: (question) => onChange({ question }),
    onSql: setSql,
    onRunRequested: (sql, task) => runAndRemember(sql, generation.question, task),
  });

  const run = () => {
    if (!generation.requestRun()) runAndRemember(tab.sql, generation.question);
  };

  /** ↵ in the Ask box: /explain, /fix and /improve work on the editor's SQL; anything else runs. */
  const submit = () => {
    const plan = planRequest(generation.question, skills, { previous: lastRun, sql: tab.sql });
    if (plan.kind === "draft") return run();
    generation.act(plan.command, tab.sql, plan.command === "fix" ? query.error : null);
  };

  // From the first-run setup: ask the tab's question as soon as it shows, then run the answer.
  const runWhenAnswered = useRef(false);
  useEffect(() => {
    if (active && tab.autoAsk && schema) {
      onChange({ autoAsk: false });
      runWhenAnswered.current = true;
      generation.setQuestion(tab.question);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, tab.autoAsk, schema]);
  useEffect(() => {
    const kind = generation.status.kind;
    if (!runWhenAnswered.current || kind === "idle" || kind === "writing") return;
    runWhenAnswered.current = false;
    // A write would still stop at the confirmation dialog; an error shows in the Ask box.
    if (kind === "done" || kind === "instant") runAndRemember(tab.sql, generation.question);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [generation.status]);

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
  const canUseAi = settings.aiReady && !!schema;
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
            sql={tab.sql}
            onSqlChange={setSql}
            onRun={run}
            onSubmit={submit}
            skills={skills}
            onCreateSkill={onCreateSkill}
            explanation={generation.explanation}
            onDismissExplanation={generation.dismissExplanation}
            running={query.running}
            onImprove={() => generation.improve(tab.sql)}
            canImprove={canUseAi && !busy && !!tab.sql.trim()}
            onSave={onSave}
            onOpenSettings={onOpenSettings}
            onRetry={generation.retry}
            followUp={lastRun ? { onStartFresh: () => onChange({ lastRun: null }) } : undefined}
            focusTables={tab.focusTables ?? []}
            focusEnabled={focusEnabled}
            onFocusEnabledChange={(enabled) => onChange({ focusEnabled: enabled })}
            onFocusTablesChange={(focusTables) => onChange({ focusTables })}
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
