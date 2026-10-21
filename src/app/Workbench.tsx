import { useEffect, useRef, useState } from "react";
import { Database, FileCode2, Server, Table2 } from "lucide-react";
import { commands, type Bootstrap, type ConnectionConfig, type Script, type Table, type Target } from "../api";
import { Alert, Button, Dialog, EmptyState, ErrorBoundary, SplitPane, TabBar, type TabItem } from "../components/ui";
import { ConnectionDialog, DATABASE_KINDS, newConnection, targetKey, useConnections } from "../features/connections";
import { DatabaseNavigator, type NavigatorScript } from "../features/navigator";
import { CloseTabDialog, SaveScriptDialog, useScripts } from "../features/scripts";
import { selectAllSql } from "../features/schema/sql";
import { SettingsDialog, useSettings } from "../features/settings";
import { hasUnsavedChanges, useTabs, type EditorTab } from "../features/tabs";
import { useUpdate } from "../features/updates";
import { FeedbackDialog } from "../features/feedback";
import { exampleQuestions, OnboardingDialog, type OnboardingStep } from "../features/onboarding";
import { QueryTab } from "./QueryTab";
import { StatusBar } from "./StatusBar";
import { TopBar } from "./TopBar";
import styles from "./Workbench.module.css";

type OpenDialog =
  /** `fromSetup`: opened from the first-run setup, which resumes when it closes. */
  | { kind: "connection"; config: ConnectionConfig; fromSetup?: boolean }
  | { kind: "settings" }
  | { kind: "saveScript"; tabId: string; closeAfter: boolean }
  | { kind: "closeTab"; tabId: string }
  | { kind: "deleteConnection"; connection: ConnectionConfig }
  | { kind: "feedback" }
  | { kind: "deleteScript"; script: Script }
  | null;

/** The main window. Owns no logic of its own: it wires the feature hooks to the feature views. */
export function Workbench({ boot }: { boot: Bootstrap }) {
  const conn = useConnections(boot.connections);
  const tabs = useTabs();
  const scripts = useScripts();
  const { settings, setSettings, modelNames } = useSettings(boot.settings);
  const updates = useUpdate();
  const [dialog, setDialog] = useState<OpenDialog>(null);
  // First launch: the setup shows until it's finished or dismissed. People who already
  // have an AI provider working (from before the setup existed) never see it.
  const [setupStep, setSetupStep] = useState<OnboardingStep | null>(() =>
    !boot.settings.onboarded && !boot.settings.aiReady ? "welcome" : null,
  );

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

  const tabById = (id: string) => tabs.tabs.find((t) => t.id === id) ?? null;
  const scriptOf = (t: EditorTab) => (t.scriptId ? (scripts.scripts.find((s) => s.id === t.scriptId) ?? null) : null);

  /** Saves the tab as a script (named `name`, or keeping its script's name) and marks it saved. */
  const saveTab = async (t: EditorTab, name?: string) => {
    const existing = scriptOf(t);
    const saved = await scripts.save({
      id: existing?.id ?? "",
      name: name ?? existing?.name ?? t.title,
      connectionId: t.target.connectionId,
      database: t.target.database,
      question: t.question,
      sql: t.sql,
      updatedAt: 0,
    });
    tabs.update(t.id, { scriptId: saved.id, title: saved.name, savedSql: t.sql });
  };

  /** Saves straight away when the tab already is a script; otherwise asks for a name first. */
  const requestSave = (t: EditorTab, closeAfter = false) => {
    if (!t.sql.trim()) return;
    const ask = () => setDialog({ kind: "saveScript", tabId: t.id, closeAfter });
    if (!scriptOf(t)) return ask();
    saveTab(t).then(() => closeAfter && tabs.close(t.id), ask);
  };

  /** Closes the tab, first asking to save when that would lose SQL. */
  const requestClose = (id: string) => {
    const t = tabById(id);
    if (t && hasUnsavedChanges(t)) setDialog({ kind: "closeTab", tabId: id });
    else tabs.close(id);
  };

  const openScript = (id: string) => {
    const s = scripts.scripts.find((x) => x.id === id);
    if (!s) return;
    const open = tabs.tabs.find((t) => t.scriptId === id);
    if (open) return tabs.activate(open.id);
    tabs.open(
      { connectionId: s.connectionId, database: s.database },
      { scriptId: s.id, title: s.name, sql: s.sql, question: s.question, savedSql: s.sql },
    );
  };

  const deleteScript = async (s: Script) => {
    await scripts.remove(s.id);
    // Open tabs keep their SQL, now unsaved, so nothing is lost by accident.
    tabs.tabs.filter((t) => t.scriptId === s.id).forEach((t) => tabs.update(t.id, { scriptId: undefined, savedSql: undefined }));
  };

  // ⌘T / Ctrl+T opens a new tab on the current database; ⌘S / Ctrl+S saves the active tab.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      const key = e.key.toLowerCase();
      if (key === "t") {
        e.preventDefault();
        newTab();
      } else if (key === "s" && tabs.active) {
        e.preventDefault();
        requestSave(tabs.active);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const previewTable = (t: Table) => {
    if (!target || !schema) return;
    const sql = selectAllSql(t, schema.dialect);
    tabs.open(target, { title: t.name, sql, savedSql: sql, autoRun: true });
  };

  /** SQL from the navigator in a new tab on the open database; only read-only SQL is run straight away. */
  const openSql = (title: string, sql: string, run: boolean) => {
    if (target) tabs.open(target, { title, sql, savedSql: sql, autoRun: run });
  };

  const forgetConnection = (id: string) => {
    conn.remove(id);
    tabs.closeWhere((t) => t.target.connectionId === id);
  };

  const tabLabel = (t: EditorTab) => {
    const c = conn.connection(t.target.connectionId);
    const db = c?.kind === "sqlite" ? null : conn.databaseName(t.target);
    return { name: c?.name ?? "Closed connection", db };
  };

  const tabItems: TabItem[] = tabs.tabs.map((t) => {
    const { name, db } = tabLabel(t);
    const c = conn.connection(t.target.connectionId);
    // Named tabs are table previews, unless they are saved scripts.
    const Icon = t.title && !t.scriptId ? Table2 : FileCode2;
    return {
      id: t.id,
      label: t.title || db || name,
      detail: t.title ? (db ?? name) : db ? name : undefined,
      title: [name, db, t.title].filter(Boolean).join(" / "),
      icon: <Icon size={14} color={c ? DATABASE_KINDS[c.kind].color : undefined} />,
      dirty: hasUnsavedChanges(t),
    };
  });

  const navigatorScripts: NavigatorScript[] = scripts.scripts.map((s) => {
    const c = conn.connection(s.connectionId);
    return {
      id: s.id,
      name: s.name,
      location: c?.kind === "sqlite" ? c.name : (s.database ?? c?.database ?? c?.name ?? ""),
      orphaned: !c,
    };
  });

  /** A name to suggest when a tab is saved for the first time. */
  const suggestedName = (t: EditorTab) => t.title || t.question.trim().slice(0, 60) || `${tabLabel(t).db ?? tabLabel(t).name} query`;

  const openSettings = () => setDialog({ kind: "settings" });

  const finishSetup = () => {
    setSetupStep(null);
    commands.finishOnboarding().then(setSettings, () => {});
  };

  /** Ends the setup with a tab that asks `question` and runs the answer. */
  const trySetupQuestion = (question: string) => {
    finishSetup();
    if (target) tabs.open(target, { question, autoAsk: true });
  };
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
              onDeleteConnection={(c) => setDialog({ kind: "deleteConnection", connection: c })}
              onRefresh={() => target && conn.ensure(target, true)}
              onPreviewTable={previewTable}
              onNewTab={(connectionId, database) => {
                const c = conn.connection(connectionId);
                if (c) tabs.open({ connectionId, database: database ?? (c.database.trim() || null) });
              }}
              onOpenSql={openSql}
              selectedTables={tabs.active?.focusTables ?? []}
              // A new pick is meant to be used, so it switches a hidden selection back on.
              onSelectTables={(focusTables) => tabs.active && tabs.update(tabs.active.id, { focusTables, focusEnabled: true })}
              scripts={navigatorScripts}
              onOpenScript={openScript}
              onDeleteScript={(id) => {
                const s = scripts.scripts.find((x) => x.id === id);
                if (s) setDialog({ kind: "deleteScript", script: s });
              }}
            />
          }
          second={
            <div className={styles.editorArea}>
              <TabBar
                items={tabItems}
                activeId={tabs.active?.id ?? null}
                onSelect={tabs.activate}
                onClose={requestClose}
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
                      onSave={() => requestSave(t)}
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
        aiReady={settings.aiReady}
        updates={updates}
        onFeedback={() => setDialog({ kind: "feedback" })}
      />

      {dialog?.kind === "connection" && (
        <ConnectionDialog
          initial={dialog.config}
          onClose={() => {
            setDialog(null);
            if (dialog.fromSetup) setSetupStep("database");
          }}
          onSaved={(c) => {
            setDialog(null);
            conn.upsert(c);
            openConnection(c);
            if (dialog.fromSetup) setSetupStep("try");
          }}
          onDeleted={(id) => {
            setDialog(null);
            forgetConnection(id);
          }}
        />
      )}
      {dialog?.kind === "saveScript" && tabById(dialog.tabId) && (
        <SaveScriptDialog
          initialName={suggestedName(tabById(dialog.tabId)!)}
          onCancel={() => setDialog(null)}
          onSave={async (name) => {
            await saveTab(tabById(dialog.tabId)!, name);
            if (dialog.closeAfter) tabs.close(dialog.tabId);
            setDialog(null);
          }}
        />
      )}
      {dialog?.kind === "closeTab" && tabById(dialog.tabId) && (
        <CloseTabDialog
          tabName={tabItems.find((i) => i.id === dialog.tabId)?.label ?? "this tab"}
          onCancel={() => setDialog(null)}
          onDiscard={() => {
            tabs.close(dialog.tabId);
            setDialog(null);
          }}
          onSave={() => {
            setDialog(null);
            requestSave(tabById(dialog.tabId)!, true);
          }}
        />
      )}
      {dialog?.kind === "deleteScript" && (
        <Dialog
          title={`Delete “${dialog.script.name}”?`}
          size="sm"
          onClose={() => setDialog(null)}
          actions={
            <>
              <Button onClick={() => setDialog(null)}>Cancel</Button>
              <Button
                variant="danger"
                onClick={() => {
                  void deleteScript(dialog.script);
                  setDialog(null);
                }}
              >
                Delete
              </Button>
            </>
          }
        >
          <p style={{ margin: 0 }}>The saved script is removed. If it’s open in a tab, the tab keeps its SQL as unsaved.</p>
        </Dialog>
      )}
      {dialog?.kind === "deleteConnection" && (
        <Dialog
          title={`Delete “${dialog.connection.name}”?`}
          size="sm"
          onClose={() => setDialog(null)}
          actions={
            <>
              <Button onClick={() => setDialog(null)}>Cancel</Button>
              <Button
                variant="danger"
                onClick={async () => {
                  const { id } = dialog.connection;
                  setDialog(null);
                  await commands.deleteConnection(id);
                  forgetConnection(id);
                }}
              >
                Delete
              </Button>
            </>
          }
        >
          <p style={{ margin: 0 }}>
            The saved connection and its password are removed from SayQL, and its tabs close. The database itself isn’t touched.
          </p>
        </Dialog>
      )}
      {dialog?.kind === "feedback" && <FeedbackDialog onClose={() => setDialog(null)} />}
      {dialog?.kind === "settings" && (
        <SettingsDialog
          settings={settings}
          onShowSetup={() => {
            setDialog(null);
            setSetupStep("welcome");
          }}
          onClose={() => setDialog(null)}
          onSaved={(s) => {
            setDialog(null);
            setSettings(s);
          }}
        />
      )}
      {setupStep && !dialog && (
        <OnboardingDialog
          settings={settings}
          step={setupStep}
          onStep={setSetupStep}
          onSettingsSaved={setSettings}
          demoAvailable={!!conn.connection("demo")}
          onUseDemo={() => {
            const demo = conn.connection("demo");
            if (demo) openConnection(demo);
            setSetupStep("try");
          }}
          onConnectOwn={() => {
            setSetupStep(null);
            setDialog({ kind: "connection", config: newConnection(), fromSetup: true });
          }}
          examples={exampleQuestions(schema, active?.id === "demo")}
          onTry={trySetupQuestion}
          onOpenSettings={openSettings}
          onFinish={finishSetup}
        />
      )}
    </div>
  );
}
