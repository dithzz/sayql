import { useEffect, useRef, useState } from "react";
import { Database, FileCode2, Server, Table2 } from "lucide-react";
import type { Bootstrap, ConnectionConfig, Table, Target } from "../api";
import { Alert, Button, EmptyState, ErrorBoundary, SplitPane, TabBar, type TabItem } from "../components/ui";
import { ConnectionDialog, DATABASE_KINDS, newConnection, targetKey, useConnections } from "../features/connections";
import { DatabaseNavigator } from "../features/navigator";
import { selectAllSql } from "../features/schema/sql";
import { SettingsDialog, useSettings } from "../features/settings";
import { useTabs, type EditorTab } from "../features/tabs";
import { QueryTab } from "./QueryTab";
import { StatusBar } from "./StatusBar";
import { TopBar } from "./TopBar";
import styles from "./Workbench.module.css";

type OpenDialog = { kind: "connection"; config: ConnectionConfig } | { kind: "settings" } | null;

/** The main window. Owns no logic of its own: it wires the feature hooks to the feature views. */
export function Workbench({ boot }: { boot: Bootstrap }) {
  const conn = useConnections(boot.connections);
  const tabs = useTabs();
  const { settings, setSettings, modelNames } = useSettings(boot.settings);
  const [dialog, setDialog] = useState<OpenDialog>(null);

  // The navigator follows the active tab, like DBeaver's "link with editor".
  const target = tabs.active?.target ?? null;
  const active = target ? conn.connection(target.connectionId) : null;
  const { schema, loading, error } = conn.schemaState(target);
  const databaseName = conn.databaseName(target);

  // On start: drop restored tabs whose connection no longer exists, and open the first
  // connection if nothing is open. The ref keeps StrictMode's double effect from opening two.
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    tabs.closeWhere((t) => !conn.connection(t.target.connectionId));
    if (tabs.tabs.length === 0 && conn.connections[0]) openConnection(conn.connections[0]);
    // Only on start.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Load the schema of whatever tab is showing (also after its connection was edited).
  useEffect(() => {
    if (target) conn.ensure(target);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [targetKey(target), conn.connections]);

  /** Focuses a tab already open on `t`, or opens a new one. */
  const showTarget = (t: Target) => {
    const name = conn.databaseName(t);
    const existing = tabs.tabs.find(
      (x) => x.target.connectionId === t.connectionId && conn.databaseName(x.target) === name && !x.title,
    );
    if (existing) tabs.activate(existing.id);
    else tabs.open(t);
  };

  const openConnection = (c: ConnectionConfig) => showTarget({ connectionId: c.id, database: c.database.trim() || null });

  const newTab = () => {
    if (target) tabs.open(target);
    else if (conn.connections[0]) openConnection(conn.connections[0]);
  };

  // ⌘T / Ctrl+T opens a new tab on the current database.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "t") {
        e.preventDefault();
        newTab();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const previewTable = (t: Table) => {
    if (!target || !schema) return;
    tabs.open(target, { title: t.name, sql: selectAllSql(t, schema.dialect), autoRun: true });
  };

  const tabLabel = (t: EditorTab) => {
    const c = conn.connection(t.target.connectionId);
    const db = c?.kind === "sqlite" ? null : conn.databaseName(t.target);
    return { name: c?.name ?? "Closed connection", db };
  };

  const tabItems: TabItem[] = tabs.tabs.map((t) => {
    const { name, db } = tabLabel(t);
    const c = conn.connection(t.target.connectionId);
    const Icon = t.title ? Table2 : FileCode2;
    return {
      id: t.id,
      label: t.title || db || name,
      detail: t.title ? (db ?? name) : db ? name : undefined,
      title: [name, db, t.title].filter(Boolean).join(" / "),
      icon: <Icon size={14} color={c ? DATABASE_KINDS[c.kind].color : undefined} />,
    };
  });

  const openSettings = () => setDialog({ kind: "settings" });
  const openNewConnection = () => setDialog({ kind: "connection", config: newConnection() });

  return (
    <div className={styles.workbench}>
      <TopBar
        active={active}
        databaseName={databaseName}
        schema={schema}
        onNewConnection={openNewConnection}
        onOpenSettings={openSettings}
      />

      {error && target && (
        <div className={styles.banner}>
          <Alert
            tone="danger"
            actions={
              <Button size="sm" variant="ghost" onClick={() => conn.dismissError(target)}>
                Dismiss
              </Button>
            }
          >
            {error}
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
              activeId={active?.id ?? null}
              databases={conn.databases}
              databaseName={databaseName}
              schema={schema}
              loading={loading}
              onOpen={(id) => {
                const c = conn.connection(id);
                if (c && id !== active?.id) openConnection(c);
              }}
              onOpenDatabase={(database) => active && showTarget({ connectionId: active.id, database })}
              onAdd={openNewConnection}
              onEdit={(c) => setDialog({ kind: "connection", config: c })}
              onRefresh={() => target && conn.ensure(target, true)}
              onPreviewTable={previewTable}
            />
          }
          second={
            <div className={styles.editorArea}>
              <TabBar
                items={tabItems}
                activeId={tabs.active?.id ?? null}
                onSelect={tabs.activate}
                onClose={tabs.close}
                onAdd={conn.connections.length ? newTab : undefined}
                addLabel="New SQL tab (⌘T)"
              />
              <div className={styles.tabBody}>
                {tabs.tabs.map((t) => {
                  const { name, db } = tabLabel(t);
                  return (
                    <ErrorBoundary key={t.id} label="This tab" hidden={t.id !== tabs.active?.id}>
                    <QueryTab
                      tab={t}
                      active={t.id === tabs.active?.id}
                      label={db ? `${name} · ${db}` : name}
                      schema={conn.schemaState(t.target).schema}
                      settings={settings}
                      modelNames={modelNames}
                      onChange={(patch) => tabs.update(t.id, patch)}
                      onOpenSettings={openSettings}
                    />
                    </ErrorBoundary>
                  );
                })}
                {tabs.tabs.length === 0 && (
                  <EmptyState
                    icon={conn.connections.length ? Database : Server}
                    title={conn.connections.length ? "No tabs open" : "No connections yet"}
                    action={
                      conn.connections.length ? (
                        <Button variant="primary" onClick={newTab}>
                          New SQL tab
                        </Button>
                      ) : (
                        <Button variant="primary" onClick={openNewConnection}>
                          New connection
                        </Button>
                      )
                    }
                  >
                    {conn.connections.length
                      ? "Pick a database in the Database Navigator, or open a new tab."
                      : "Add a connection to get started."}
                  </EmptyState>
                )}
              </div>
            </div>
          }
        />
      </main>

      <StatusBar
        active={active}
        databaseName={databaseName}
        schema={schema}
        loading={loading}
        modelNames={modelNames}
        hasApiKey={settings.hasApiKey}
      />

      {dialog?.kind === "connection" && (
        <ConnectionDialog
          initial={dialog.config}
          onClose={() => setDialog(null)}
          onSaved={(c) => {
            setDialog(null);
            conn.upsert(c);
            openConnection(c);
          }}
          onDeleted={(id) => {
            setDialog(null);
            conn.remove(id);
            tabs.closeWhere((t) => t.target.connectionId === id);
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
    </div>
  );
}
