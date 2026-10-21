import type { RefObject } from "react";
import { Eraser, FileCode2, Play } from "lucide-react";
import type { Schema } from "../api";
import { Button, IconButton, Panel } from "../components/ui";
import { AskBar, type GenerationStatus } from "../features/ask";
import { SqlEditor } from "../features/editor";
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
  modelName: string;
  examples?: string[];
  sql: string;
  onSqlChange: (sql: string) => void;
  onRun: () => void;
  running: boolean;
  onOpenSettings: () => void;
  onRetry: () => void;
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
          <IconButton icon={Eraser} label="Clear" size="sm" onClick={() => p.onSqlChange("")} disabled={!p.sql} />
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
        modelName={p.modelName}
        disabled={!p.schema}
        examples={p.examples}
        onOpenSettings={p.onOpenSettings}
        onRetry={p.onRetry}
      />
      <div className={styles.editor}>
        <SqlEditor value={p.sql} onChange={p.onSqlChange} onRun={p.onRun} schema={p.schema} active={p.active} />
      </div>
    </Panel>
  );
}
