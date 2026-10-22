import { useEffect, useRef, useState } from "react";
import { Wand2 } from "lucide-react";
import { QUERY_CANCELLED, type Schema, type Settings, type Skill } from "../api";
import { Button, SplitPane } from "../components/ui";
import { parseCommand, planRequest, useSqlGeneration, type TaskKind } from "../features/ask";
import { resolveSelection, type TextRange } from "../features/editor";
import { statementRanges } from "../lib/sqlStatements";
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
  /** Where the selection is in the editor; Run runs just that. */
  const [selection, setSelection] = useState<TextRange | null>(null);
  /** A selection that cut a statement ran as whole statements: these, selected to show it. */
  const [widened, setWidened] = useState<{ range: TextRange; count: number } | null>(null);
  /** The last run was a selection: the AI fixes whole queries, so it stays out of it. */
  const ranSelection = useRef(false);

  const query = useQueryRunner(tab.target, (sql, error) => {
    if (ranSelection.current || autoFixUsed.current || !settings.aiReady || !generation.question.trim()) return;
    autoFixUsed.current = true;
    generation.fix(sql, error);
  });

  /** Runs `sql` and remembers it as the query follow-ups build on. */
  const runAndRemember = (sql: string, question: string, task: TaskKind | "manual" = "manual") => {
    if (task !== "fix") autoFixUsed.current = false;
    ranSelection.current = false;
    setWidened(null);
    // Follow-ups see the question without its "/command".
    onChange({ lastRun: sql.trim() ? { question: parseCommand(question, skills).rest, sql } : null });
    query.run(sql, schema?.dialect);
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

  /**
   * Runs just the selected SQL, if any. A selection that cuts a statement (a DELETE without
   * its WHERE) runs the whole statements it touches instead, selected in the editor to show
   * it. Follow-ups keep building on the whole query, since the AI rewrites the whole editor.
   */
  const runSelectionOrAll = () => {
    const r = selection && resolveSelection(tab.sql, selection, schema?.dialect);
    if (!r) return run();
    ranSelection.current = true;
    setWidened(r.expanded ? { range: r.range, count: statementRanges(r.sql, schema?.dialect).length } : null);
    query.run(r.sql, schema?.dialect);
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
            modelNames={modelNames}
            onOpenSettings={onOpenSettings}
            skills={{ list: skills, onCreate: onCreateSkill }}
            generation={{
              askRef,
              question: generation.question,
              onQuestionChange: generation.setQuestion,
              status: generation.status,
              onSubmit: submit,
              onRetry: generation.retry,
              onImprove: () => generation.improve(tab.sql),
              canImprove: canUseAi && !busy && !!tab.sql.trim(),
              followUp: lastRun ? { onStartFresh: () => onChange({ lastRun: null }) } : undefined,
              explanation: generation.explanation,
              onDismissExplanation: generation.dismissExplanation,
            }}
            query={{
              sql: tab.sql,
              onSqlChange: setSql,
              onRun: runSelectionOrAll,
              running: query.running,
              onCancel: query.cancel,
              selection,
              onSelectionChange: setSelection,
              select: widened?.range ?? null,
              onSave,
            }}
            focus={{
              tables: tab.focusTables ?? [],
              enabled: focusEnabled,
              onEnabledChange: (enabled) => onChange({ focusEnabled: enabled }),
              onTablesChange: (focusTables) => onChange({ focusTables }),
            }}
            origin={tab.origin}
          />
        }
        second={
          <ResultsPanel
            outcomes={query.outcomes}
            total={query.total}
            running={query.running}
            note={widened ? `Ran the full statement${widened.count > 1 ? "s" : ""}` : undefined}
            errorHint={
              autoFixFailed
                ? `${modelNames.expert} couldn't fix it automatically. Try rephrasing your question, or edit the SQL above.`
                : undefined
            }
            errorAction={
              // The AI rewrites one whole query, so a selection or a script is left to the person.
              canUseAi && !ranSelection.current && query.total === 1 && query.error !== QUERY_CANCELLED && (
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
