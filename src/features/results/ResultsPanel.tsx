import { useState, type ReactNode } from "react";
import { Check, CheckCircle2, Copy, Table2 } from "lucide-react";
import type { QueryResult } from "../../api";
import { Alert, Badge, Button, EmptyState, Panel, Spinner } from "../../components/ui";
import { DataGrid } from "./DataGrid";
import { plural, toCsv } from "./format";
import styles from "./ResultsPanel.module.css";

export interface ResultsPanelProps {
  result: QueryResult | null;
  error: string | null;
  running: boolean;
  /** Shown under a database error, e.g. advice to rephrase. */
  errorHint?: string;
  /** Shown next to a database error, e.g. a Fix with AI button. */
  errorAction?: ReactNode;
}

export function ResultsPanel({ result, error, running, errorHint, errorAction }: ResultsPanelProps) {
  const [copied, setCopied] = useState(false);
  const hasGrid = !!result && result.columns.length > 0;

  const copyCsv = async () => {
    if (!result) return;
    await navigator.clipboard.writeText(toCsv(result));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <Panel
      title="Results"
      icon={Table2}
      subtitle={
        running ? (
          <span className={styles.meta}>
            <Spinner size={12} /> Running…
          </span>
        ) : (
          result &&
          !error && (
            <span className={styles.meta}>
              {hasGrid ? plural(result.rows.length, "row") : plural(result.rowsAffected ?? 0, "row") + " affected"}
              {" · "}
              {result.elapsedMs} ms
              {result.truncated && <Badge tone="warning">first 5,000 rows</Badge>}
            </span>
          )
        )
      }
      actions={
        hasGrid && (
          <Button size="sm" variant="ghost" icon={copied ? Check : Copy} onClick={copyCsv}>
            {copied ? "Copied" : "Copy as CSV"}
          </Button>
        )
      }
    >
      {error ? (
        <div className={styles.error}>
          <Alert tone="danger" actions={errorAction}>
            <strong>The database returned an error</strong>
            <pre className={styles.errorSql}>{error}</pre>
          </Alert>
          {errorHint && <span className={styles.meta}>{errorHint}</span>}
        </div>
      ) : hasGrid ? (
        <DataGrid result={result} />
      ) : result ? (
        <EmptyState icon={CheckCircle2} title="Statement executed">
          {plural(result.rowsAffected ?? 0, "row")} affected.
        </EmptyState>
      ) : (
        <EmptyState icon={Table2} title="No results yet">
          Ask a question or write SQL, then press Run.
        </EmptyState>
      )}
    </Panel>
  );
}
