import type { RefObject } from "react";
import { Eraser, FileCode2, Play, Save, Wand2 } from "lucide-react";
import type { Schema, Skill } from "../api";
import { Badge, Button, IconButton, Panel } from "../components/ui";
import { AskBar, type GenerationStatus } from "../features/ask";
import { voicePrompt } from "../features/voice";
import { SqlEditor, type TextRange } from "../features/editor";
import type { ModelNames } from "../features/settings";
import styles from "./Workbench.module.css";

export interface SqlWorkspaceProps {
  connectionName: string;
  schema: Schema | null;
  /** Whether this is the visible tab. */
  active: boolean;
  askRef: RefObject<HTMLTextAreaElement | null>;
  question: string;
  onQuestionChange: (q: string) => void;
  status: GenerationStatus;
  modelNames: ModelNames;
  /** Asks the expert model to review the SQL. */
  onImprove: () => void;
  canImprove: boolean;
  /** Saves the tab as a script (⌘S). */
  onSave: () => void;
  sql: string;
  onSqlChange: (sql: string) => void;
  /** Runs the selected SQL if there is any, else all of it (Run button, ⌘↵ in the editor). */
  onRun: () => void;
  /** Where the selection is in the editor; Run runs just that (snapped to whole statements). */
  selection: TextRange | null;
  onSelectionChange: (selection: TextRange | null) => void;
  /** Shows what ran when a selection was widened. */
  select: TextRange | null;
  running: boolean;
  onOpenSettings: () => void;
  onRetry: () => void;
  followUp?: { onStartFresh: () => void };
  /** ↵ in the Ask box: run, or the picked /explain, /fix or /improve. */
  onSubmit: () => void;
  skills: Skill[];
  onCreateSkill: () => void;
  explanation: string | null;
  onDismissExplanation: () => void;
  focusTables: string[];
  focusEnabled: boolean;
  onFocusEnabledChange: (enabled: boolean) => void;
  onFocusTablesChange: (names: string[]) => void;
  /** Opened by an AI app, e.g. "From Claude (MCP)". */
  origin?: string;
}

/** The SQL editor view: the plain-English Ask bar above an editable SQL editor. */
export function SqlWorkspace(p: SqlWorkspaceProps) {
  const writing = p.status.kind === "writing";
  const hasSelection = !!p.selection && !!p.sql.slice(p.selection.start, p.selection.end).trim();
  return (
    <Panel
      title="SQL Editor"
      icon={FileCode2}
      subtitle={p.connectionName}
      actions={
        <>
          {p.origin && (
            <Badge tone="ai" title="Opened by an AI app. Nothing has run.">
              {p.origin}
            </Badge>
          )}
          <IconButton icon={Save} label="Save script (⌘S)" size="sm" onClick={p.onSave} disabled={!p.sql.trim()} />
          <IconButton icon={Eraser} label="Clear" size="sm" onClick={() => p.onSqlChange("")} disabled={!p.sql} />
          <Button
            size="sm"
            variant="ghost"
            icon={Wand2}
            onClick={p.onImprove}
            disabled={!p.canImprove}
            title={`Review and improve this query with ${p.modelNames.expert}`}
          >
            Improve
          </Button>
          <Button
            size="sm"
            variant="primary"
            icon={Play}
            shortcut="⌘↵"
            onClick={p.onRun}
            loading={p.running}
            disabled={!p.schema || (!p.sql.trim() && !writing)}
            title={hasSelection ? "Run only the selected statements" : undefined}
          >
            {hasSelection ? "Run selection" : writing ? "Run when ready" : "Run"}
          </Button>
        </>
      }
    >
      <AskBar
        ref={p.askRef}
        question={p.question}
        onQuestionChange={p.onQuestionChange}
        onSubmit={p.onSubmit}
        status={p.status}
        modelNames={p.modelNames}
        disabled={!p.schema}
        onOpenSettings={p.onOpenSettings}
        onRetry={p.onRetry}
        followUp={p.followUp}
        skills={p.skills}
        onCreateSkill={p.onCreateSkill}
        explanation={p.explanation}
        onDismissExplanation={p.onDismissExplanation}
        focusTables={p.focusTables}
        focusEnabled={p.focusEnabled}
        onFocusEnabledChange={p.onFocusEnabledChange}
        onFocusTablesChange={p.onFocusTablesChange}
        active={p.active}
        voicePrompt={voicePrompt(p.schema, p.focusTables)}
      />
      <div className={styles.editor}>
        <SqlEditor
          value={p.sql}
          onChange={p.onSqlChange}
          onRun={p.onRun}
          onSelectionChange={p.onSelectionChange}
          select={p.select}
          schema={p.schema}
          active={p.active}
        />
      </div>
    </Panel>
  );
}
