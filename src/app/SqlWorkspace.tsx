import type { RefObject } from "react";
import { Eraser, FileCode2, Play, Save, Wand2 } from "lucide-react";
import type { Schema, Skill } from "../api";
import { Button, IconButton, Panel } from "../components/ui";
import { AskBar, type GenerationStatus } from "../features/ask";
import { SqlEditor, type TextRange } from "../features/editor";
import type { ModelNames } from "../features/settings";
import styles from "./Workbench.module.css";

/** The Ask box: the question and the AI's work on it. */
export interface WorkspaceGeneration {
  askRef: RefObject<HTMLTextAreaElement | null>;
  question: string;
  onQuestionChange: (q: string) => void;
  status: GenerationStatus;
  /** ↵ in the Ask box: run, or the picked /explain, /fix or /improve. */
  onSubmit: () => void;
  onRetry: () => void;
  /** Asks the expert model to review the SQL. */
  onImprove: () => void;
  canImprove: boolean;
  followUp?: { onStartFresh: () => void };
  explanation: string | null;
  onDismissExplanation: () => void;
}

/** The SQL editor and running it. */
export interface WorkspaceQuery {
  sql: string;
  onSqlChange: (sql: string) => void;
  /** Runs the selected SQL if there is any, else all of it (Run button, ⌘↵ in the editor). */
  onRun: () => void;
  running: boolean;
  /** Where the selection is in the editor; Run runs just that (snapped to whole statements). */
  selection: TextRange | null;
  onSelectionChange: (selection: TextRange | null) => void;
  /** Shows what ran when a selection was widened. */
  select: TextRange | null;
  /** Saves the tab as a script (⌘S). */
  onSave: () => void;
}

/** Tables picked in the navigator as AI context. */
export interface WorkspaceFocus {
  tables: string[];
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  onTablesChange: (names: string[]) => void;
}

export interface SqlWorkspaceProps {
  connectionName: string;
  schema: Schema | null;
  /** Whether this is the visible tab. */
  active: boolean;
  modelNames: ModelNames;
  onOpenSettings: () => void;
  skills: { list: Skill[]; onCreate: () => void };
  generation: WorkspaceGeneration;
  query: WorkspaceQuery;
  focus: WorkspaceFocus;
}

/** The SQL editor view: the plain-English Ask bar above an editable SQL editor. */
export function SqlWorkspace({ connectionName, schema, active, modelNames, onOpenSettings, skills, generation: g, query: q, focus }: SqlWorkspaceProps) {
  const writing = g.status.kind === "writing";
  const hasSelection = !!q.selection && !!q.sql.slice(q.selection.start, q.selection.end).trim();
  return (
    <Panel
      title="SQL Editor"
      icon={FileCode2}
      subtitle={connectionName}
      actions={
        <>
          <IconButton icon={Save} label="Save script (⌘S)" size="sm" onClick={q.onSave} disabled={!q.sql.trim()} />
          <IconButton icon={Eraser} label="Clear" size="sm" onClick={() => q.onSqlChange("")} disabled={!q.sql} />
          <Button
            size="sm"
            variant="ghost"
            icon={Wand2}
            onClick={g.onImprove}
            disabled={!g.canImprove}
            title={`Review and improve this query with ${modelNames.expert}`}
          >
            Improve
          </Button>
          <Button
            size="sm"
            variant="primary"
            icon={Play}
            shortcut="⌘↵"
            onClick={q.onRun}
            loading={q.running}
            disabled={!schema || (!q.sql.trim() && !writing)}
            title={hasSelection ? "Run only the selected statements" : undefined}
          >
            {hasSelection ? "Run selection" : writing ? "Run when ready" : "Run"}
          </Button>
        </>
      }
    >
      <AskBar
        ref={g.askRef}
        question={g.question}
        onQuestionChange={g.onQuestionChange}
        onSubmit={g.onSubmit}
        status={g.status}
        modelNames={modelNames}
        disabled={!schema}
        onOpenSettings={onOpenSettings}
        onRetry={g.onRetry}
        followUp={g.followUp}
        skills={skills.list}
        onCreateSkill={skills.onCreate}
        explanation={g.explanation}
        onDismissExplanation={g.onDismissExplanation}
        focusTables={focus.tables}
        focusEnabled={focus.enabled}
        onFocusEnabledChange={focus.onEnabledChange}
        onFocusTablesChange={focus.onTablesChange}
      />
      <div className={styles.editor}>
        <SqlEditor
          value={q.sql}
          onChange={q.onSqlChange}
          onRun={q.onRun}
          onSelectionChange={q.onSelectionChange}
          select={q.select}
          schema={schema}
          active={active}
        />
      </div>
    </Panel>
  );
}
