import type { RefObject } from "react";
import { Eraser, FileCode2, Play, Save, Wand2 } from "lucide-react";
import type { Schema } from "../api";
import { Button, IconButton, Panel } from "../components/ui";
import { AskBar, type GenerationStatus } from "../features/ask";
import { SqlEditor } from "../features/editor";
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
  examples?: string[];
  sql: string;
  onSqlChange: (sql: string) => void;
  onRun: () => void;
  running: boolean;
  onOpenSettings: () => void;
  onRetry: () => void;
  followUp?: { onStartFresh: () => void };
  focusTables: string[];
  onFocusTablesChange: (names: string[]) => void;
}

/** The SQL editor view: the plain-English Ask bar above an editable SQL editor. */
export function SqlWorkspace(p: SqlWorkspaceProps) {
  const writing = p.status.kind === "writing";
  return (
    <Panel
      title="SQL Editor"
      icon={FileCode2}
      subtitle={p.connectionName}
      actions={
        <>
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
          >
            {writing ? "Run when ready" : "Run"}
          </Button>
        </>
      }
    >
      <AskBar
        ref={p.askRef}
        question={p.question}
        onQuestionChange={p.onQuestionChange}
        onSubmit={p.onRun}
        status={p.status}
        modelNames={p.modelNames}
        disabled={!p.schema}
        examples={p.examples}
        onOpenSettings={p.onOpenSettings}
        onRetry={p.onRetry}
        followUp={p.followUp}
        focusTables={p.focusTables}
        onFocusTablesChange={p.onFocusTablesChange}
      />
      <div className={styles.editor}>
        <SqlEditor value={p.sql} onChange={p.onSqlChange} onRun={p.onRun} schema={p.schema} active={p.active} />
      </div>
    </Panel>
  );
}
