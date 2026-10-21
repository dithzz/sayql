import { useEffect, useRef, useState } from "react";
import { Server } from "lucide-react";
import type { Bootstrap, ConnectionConfig, Table } from "../api";
import { Alert, Button, EmptyState, SplitPane } from "../components/ui";
import { DEMO_EXAMPLES, useSqlGeneration } from "../features/ask";
import { ConnectionDialog, newConnection, useConnections } from "../features/connections";
import { DatabaseNavigator } from "../features/navigator";
import { ConfirmWriteDialog, useQueryRunner } from "../features/query";
import { ResultsPanel } from "../features/results";
import { selectAllSql } from "../features/schema/sql";
import { SettingsDialog, useSettings } from "../features/settings";
import { SqlWorkspace } from "./SqlWorkspace";
import { StatusBar } from "./StatusBar";
import { TopBar } from "./TopBar";
import styles from "./Workbench.module.css";

const DEMO_CONNECTION_ID = "demo";

type OpenDialog = { kind: "connection"; config: ConnectionConfig } | { kind: "settings" } | null;

/** The main window. Owns no logic of its own: it wires the feature hooks to the feature views. */
export function Workbench({ boot }: { boot: Bootstrap }) {
  const conn = useConnections(boot.connections);
  const { settings, setSettings, modelName } = useSettings(boot.settings);
  const [sql, setSql] = useState("");
  const [dialog, setDialog] = useState<OpenDialog>(null);
  const askRef = useRef<HTMLTextAreaElement>(null);
  const connectionId = conn.active?.id ?? null;

  const query = useQueryRunner(connectionId);
  const generation = useSqlGeneration({
    connectionId,
    schema: conn.schema,
    model: settings.model,
    hasApiKey: settings.hasApiKey,
    onSql: setSql,
    onRunRequested: query.run,
  });

  useEffect(() => setSql(""), [connectionId]);
  useEffect(() => {
    if (conn.schema) askRef.current?.focus();
  }, [conn.schema]);

  const run = () => {
    if (!generation.requestRun()) query.run(sql);
  };

  const previewTable = (t: Table) => {
    if (!conn.schema) return;
    const preview = selectAllSql(t, conn.schema.dialect);
    generation.setQuestion("");
    setSql(preview);
    query.run(preview);
  };

  const openSettings = () => setDialog({ kind: "settings" });
  const openNewConnection = () => setDialog({ kind: "connection", config: newConnection() });

  return (
    <div className={styles.workbench}>
      <TopBar active={conn.active} schema={conn.schema} onNewConnection={openNewConnection} onOpenSettings={openSettings} />

      {conn.error && (
        <div className={styles.banner}>
          <Alert
            tone="danger"
            actions={
              <Button size="sm" variant="ghost" onClick={conn.dismissError}>
                Dismiss
              </Button>
            }
          >
            {conn.error}
          </Alert>
        </div>
      )}

      <main className={styles.body}>
        <SplitPane
          direction="horizontal"
          persistKey="navigator"
          initialSize={280}
          minSize={200}
          maxSize={560}
          first={
            <DatabaseNavigator
              connections={conn.connections}
              activeId={connectionId}
              schema={conn.schema}
              loading={conn.loadingSchema}
              onOpen={conn.open}
              onAdd={openNewConnection}
              onEdit={(c) => setDialog({ kind: "connection", config: c })}
              onRefresh={conn.refresh}
              onPreviewTable={previewTable}
            />
          }
          second={
            conn.active ? (
              <SplitPane
                direction="vertical"
                persistKey="editor"
                initialSize={340}
                minSize={200}
                maxSize={900}
                first={
                  <SqlWorkspace
                    connectionName={conn.active.name}
                    schema={conn.schema}
                    askRef={askRef}
                    question={generation.question}
                    onQuestionChange={generation.setQuestion}
                    status={generation.status}
                    modelName={modelName}
                    examples={conn.active.id === DEMO_CONNECTION_ID ? DEMO_EXAMPLES : undefined}
                    sql={sql}
                    onSqlChange={setSql}
                    onRun={run}
                    running={query.running}
                    onOpenSettings={openSettings}
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
            ) : (
              <EmptyState
                icon={Server}
                title="No connection open"
                action={
                  <Button variant="primary" onClick={openNewConnection}>
                    New connection
                  </Button>
                }
              >
                Choose a connection in the Database Navigator, or add one.
              </EmptyState>
            )
          }
        />
      </main>

      <StatusBar
        active={conn.active}
        schema={conn.schema}
        loading={conn.loadingSchema}
        modelName={modelName}
        hasApiKey={settings.hasApiKey}
      />

      {dialog?.kind === "connection" && (
        <ConnectionDialog
          initial={dialog.config}
          onClose={() => setDialog(null)}
          onSaved={(c) => {
            setDialog(null);
            conn.upsert(c);
          }}
          onDeleted={(id) => {
            setDialog(null);
            conn.remove(id);
          }}
        />
      )}
      {dialog?.kind === "settings" && (
        <SettingsDialog
          settings={settings}
          onClose={() => setDialog(null)}
          onSaved={(s) => {
            setDialog(null);
            setSettings(s);
          }}
        />
      )}
      {query.pendingWrite && conn.active && (
        <ConfirmWriteDialog
          sql={query.pendingWrite}
          connectionName={conn.active.name}
          onCancel={query.cancelWrite}
          onConfirm={query.confirmWrite}
        />
      )}
    </div>
  );
}
