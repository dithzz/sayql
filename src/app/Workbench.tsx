import { useEffect, useRef, useState } from "react";
import { Copy, Database, FileCode2, Plus, Save, Server, Table2, X } from "lucide-react";
import { TEAM_SIGNED_OUT, commands, type Bootstrap, type ConnectionConfig, type DbNote, type Script, type Table, type TeamAccount, type TeamAiStatus, type Target } from "../api";
import { Alert, Button, Dialog, EmptyState, ErrorBoundary, Spinner, SplitPane, TabBar, type MenuEntry, type TabItem } from "../components/ui";
import {
  AgentAccessDialog,
  NoteDialog,
  NoteSuggestions,
  SUGGESTIONS_CHANGED,
  TeachSayql,
  AuditDialog,
  lastAudit,
  saveAgentAccess,
  suggestionFor,
  useAgentAccess,
  useAudits,
  useNoteSuggestions,
} from "../features/agent";
import { usePersistentState } from "../hooks/usePersistentState";
import { ConnectionDialog, DATABASE_KINDS, ProdBadge, newConnection, targetKey, useConnections } from "../features/connections";
import { DatabaseNavigator, type NavigatorScript } from "../features/navigator";
import { CloseTabDialog, CloseTabsDialog, SaveScriptDialog, useScripts } from "../features/scripts";
import { selectAllSql } from "../features/schema/sql";
import { SettingsPage, useSettings, type AiAppsTab, type SettingsSection } from "../features/settings";
import { useSkills } from "../features/skills";
import { hasUnsavedChanges, isPristine, useTabs, type EditorTab } from "../features/tabs";
import { useUpdate } from "../features/updates";
import { FeedbackDialog } from "../features/feedback";
import { useHandoffs } from "../features/handoff";
import { TeamSignInDialog, teamAiDestination, teamAiOn, useTeamConnections } from "../features/team";
import { exampleQuestions, OnboardingDialog, type OnboardingStep } from "../features/onboarding";
import { QueryTab } from "./QueryTab";
import { StatusBar } from "./StatusBar";
import { TopBar } from "./TopBar";
import { useConnectionActions } from "./useConnectionActions";
import { useScriptActions } from "./useScriptActions";
import styles from "./Workbench.module.css";

type OpenDialog =
  /** `fromSetup`: opened from the first-run setup, which resumes when it closes. */
  /** `note`: where the details came from, for a handoff from an AI app. */
  | { kind: "connection"; config: ConnectionConfig; fromSetup?: boolean; note?: string; key?: string }
  /** `createSkill`: opened from "Create skill…" in the Ask box. */
  | { kind: "settings"; section?: SettingsSection; aiAppsTab?: AiAppsTab; createSkill?: boolean }
  | { kind: "saveScript"; tabId: string; closeAfter: boolean }
  | { kind: "closeTab"; tabId: string }
  | { kind: "closeTabs"; tabIds: string[] }
  | { kind: "deleteConnection"; connection: ConnectionConfig }
  | { kind: "feedback" }
  | { kind: "deleteScript"; script: Script }
  | { kind: "agentAccess"; connection: ConnectionConfig }
  | { kind: "note"; note: DbNote | null }
  | { kind: "noteSuggestions" }
  /** "Teach SayQL your database", from the navigator. */
  | { kind: "teach" }
  /** "Audit with…", from "Teach SayQL your database". */
  | { kind: "audit" }
  /** From a sayql://team-join invite link. */
  | { kind: "teamSignIn"; server: string; code: string; key: string }
  | null;

/** The main window. Owns no logic of its own: it wires the feature hooks to the feature views. */
export function Workbench({ boot }: { boot: Bootstrap }) {
  const conn = useConnections(boot.connections);
  const tabs = useTabs();
  const scripts = useScripts();
  const { scriptOf, saveTab, openScript, deleteScript } = useScriptActions(scripts, tabs);
  const connActions = useConnectionActions(conn, tabs);
  const skills = useSkills();
  const { settings, setSettings, modelNames } = useSettings(boot.settings);
  const updates = useUpdate();
  const [dialog, setDialog] = useState<OpenDialog>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // First launch: the setup shows until it's finished or dismissed. People who already
  // have an AI provider working (from before the setup existed) never see it.
  const [setupStep, setSetupStep] = useState<OnboardingStep | null>(() =>
    !boot.settings.onboarded && !boot.settings.aiReady ? "welcome" : null,
  );

  /** Tabs with an agent run going, which show a spinner. */
  const [busyTabs, setBusyTabs] = useState<Set<string>>(new Set());
  const setBusy = (id: string, busy: boolean) =>
    setBusyTabs((s) => {
      if (s.has(id) === busy) return s;
      const next = new Set(s);
      if (busy) next.add(id);
      else next.delete(id);
      return next;
    });
  /** Bumped when a note changes, so the navigator lists it. */
  const [notesVersion, setNotesVersion] = useState(0);

  // The navigator follows the active tab, like DBeaver's "link with editor".
  const target = tabs.active?.target ?? null;
  const active = target ? conn.connection(target.connectionId) : null;
  const { schema, loading, error } = conn.schemaState(target);
  const databaseName = conn.databaseName(target);
  const [notes, setNotes] = useState<DbNote[]>([]);
  useEffect(() => {
    if (!target) return setNotes([]);
    commands.listNotes(target).then(setNotes, () => setNotes([]));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [targetKey(target), notesVersion]);
  // Notes AI apps suggested over MCP; an accepted one (here or in Settings) is a new note.
  const noteSuggestions = useNoteSuggestions();
  const suggestionsHere = noteSuggestions.suggestions.filter((s) => suggestionFor(s, target));
  useEffect(() => {
    const bump = () => setNotesVersion((v) => v + 1);
    window.addEventListener(SUGGESTIONS_CHANGED, bump);
    return () => window.removeEventListener(SUGGESTIONS_CHANGED, bump);
  }, []);
  // "Teach SayQL your database" shows under the open database until it's hidden or an AI app
  // has audited the connection. MCP serves the connections saved here, not a team's.
  const [teachHidden, setTeachHidden] = usePersistentState("sayql.teachHidden", false);
  const audits = useAudits();
  const teachHere = !!active && !active.id.startsWith("team:") && !teachHidden && lastAudit(audits, active.id) === null;
  const agentAccessConnection = dialog?.kind === "agentAccess" ? dialog.connection.id : null;
  const dialogAccess = useAgentAccess(agentAccessConnection);

  // SayQL Team: the team's shared connections, fetched only while signed in.
  const setTeam = (team: TeamAccount | null) => setSettings((s) => ({ ...s, team }));
  /** Team AI: the company's AI for team connections; null when off or signed out. */
  const [teamAi, setTeamAi] = useState<TeamAiStatus | null>(null);
  useTeamConnections(
    settings.team ?? null,
    conn.setTeam,
    (e) => {
      if (e === TEAM_SIGNED_OUT) setTeam(null);
      setNotice(`Couldn't load your team's connections: ${e}`);
    },
    setTeamAi,
  );
  /** Team AI for a connection: only team connections use it. */
  const teamAiFor = (connectionId: string) => (conn.connection(connectionId)?.team && teamAiOn(teamAi) ? teamAi : null);
  // A query or schema load that found the team sign-in revoked: show the team as signed out.
  useEffect(() => {
    if (error?.includes(TEAM_SIGNED_OUT)) setTeam(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [error]);

  // On start: drop restored tabs whose connection no longer exists. Nothing connects on its
  // own, not even the first connection: restored tabs wait for the person to click the
  // connection, Connect, Run or Ask (a production database is never opened silently).
  // Tabs on team connections stay while signed in: those connections load from the server.
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    const teamTab = (id: string) => !!boot.settings.team && id.startsWith("team:");
    tabs.closeWhere((t) => !conn.connection(t.target.connectionId) && !teamTab(t.target.connectionId));
    // Only on start.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Load the schema of whatever tab is showing, once its connection is connected (also after
  // the connection was edited).
  const targetConnected = !!target && conn.isConnected(target.connectionId);
  useEffect(() => {
    if (target && targetConnected) conn.ensure(target);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [targetKey(target), targetConnected, conn.connections]);

  /** Focuses a tab already open on `t`, or opens a new one, and connects (the person asked for it). */
  const showTarget = (t: Target) => {
    const name = conn.databaseName(t);
    const existing = tabs.tabs.find(
      (x) => x.target.connectionId === t.connectionId && conn.databaseName(x.target) === name && !x.title,
    );
    if (existing) tabs.activate(existing.id);
    else tabs.open(t);
    conn.connect(existing?.target ?? t);
  };

  const openConnection = (c: ConnectionConfig) => showTarget({ connectionId: c.id, database: c.database.trim() || null });

  const newTab = () => {
    if (target) tabs.open(target);
    else if (conn.connections[0]) openConnection(conn.connections[0]);
  };

  const tabById = (id: string) => tabs.tabs.find((t) => t.id === id) ?? null;

  /** Saves straight away when the tab already is a script; otherwise asks for a name first. */
  const requestSave = (t: EditorTab, closeAfter = false) => {
    if (!t.sql.trim()) return;
    const ask = () => setDialog({ kind: "saveScript", tabId: t.id, closeAfter });
    if (!scriptOf(t)) return ask();
    saveTab(t).then(() => closeAfter && tabs.close(t.id), ask);
  };

  /** Closes several tabs; asks first when some have SQL that isn't saved. */
  const requestCloseMany = (ids: string[]) => {
    if (ids.length === 1) return requestClose(ids[0]);
    const closing = new Set(ids);
    if (tabs.tabs.some((t) => closing.has(t.id) && hasUnsavedChanges(t))) setDialog({ kind: "closeTabs", tabIds: ids });
    else tabs.closeWhere((t) => closing.has(t.id));
  };

  /** A tab's right-click menu, like an editor's. */
  const tabMenu = (id: string): MenuEntry[] => {
    const all = tabs.tabs;
    const i = all.findIndex((t) => t.id === id);
    const t = all[i];
    if (!t) return [];
    const ids = (list: EditorTab[]) => list.map((x) => x.id);
    const others = all.filter((x) => x.id !== id);
    const right = all.slice(i + 1);
    const left = all.slice(0, i);
    const saved = all.filter((x) => !hasUnsavedChanges(x));
    return [
      { label: "Close", icon: X, onSelect: () => requestClose(id) },
      { label: "Close others", disabled: !others.length, onSelect: () => requestCloseMany(ids(others)) },
      { label: "Close to the right", disabled: !right.length, onSelect: () => requestCloseMany(ids(right)) },
      { label: "Close to the left", disabled: !left.length, onSelect: () => requestCloseMany(ids(left)) },
      { label: "Close saved tabs", disabled: !saved.length, onSelect: () => requestCloseMany(ids(saved)) },
      { label: "Close all", onSelect: () => requestCloseMany(ids(all)) },
      "separator",
      { label: "Duplicate", icon: Copy, onSelect: () => tabs.open(t.target, { title: t.title, question: t.question, sql: t.sql }) },
      { label: "New tab on this database", icon: Plus, onSelect: () => tabs.open(t.target) },
      { label: scriptOf(t) ? "Save" : "Save as script…", icon: Save, shortcut: "⌘S", disabled: !t.sql.trim(), onSelect: () => requestSave(t) },
    ];
  };

  /** Closes the tab, first asking to save when that would lose SQL. */
  const requestClose = (id: string) => {
    const t = tabById(id);
    if (t && hasUnsavedChanges(t)) setDialog({ kind: "closeTab", tabId: id });
    else tabs.close(id);
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

  const tabLabel = (t: EditorTab) => {
    const c = conn.connection(t.target.connectionId);
    const db = c?.kind === "sqlite" ? null : conn.databaseName(t.target);
    return { name: c?.name ?? "Closed connection", db };
  };

  const tabItems: TabItem[] = tabs.tabs.map((t) => {
    const { name, db } = tabLabel(t);
    const c = conn.connection(t.target.connectionId);
    // Named tabs are table previews, unless they are saved scripts or SQL from an AI app.
    const Icon = t.title && !t.scriptId && !t.origin ? Table2 : FileCode2;
    return {
      id: t.id,
      label: t.title || db || name,
      detail: t.title ? (db ?? name) : db ? name : undefined,
      title: [name, db, t.title].filter(Boolean).join(" / ") + (conn.isConnected(t.target.connectionId) ? "" : " (not connected)"),
      icon: busyTabs.has(t.id) ? <Spinner size={13} /> : <Icon size={14} color={c ? DATABASE_KINDS[c.kind].color : undefined} />,
      dirty: hasUnsavedChanges(t),
      badge: <ProdBadge connection={c} />,
      // Not connected yet: shown dimmed until the person connects.
      muted: !conn.isConnected(t.target.connectionId),
    };
  });

  const navigatorScripts: NavigatorScript[] = scripts.scripts.map((s) => {
    const c = conn.connection(s.connectionId);
    return {
      id: s.id,
      name: s.name,
      location: c?.kind === "sqlite" ? c.name : (s.database ?? c?.database ?? c?.name ?? ""),
      orphaned: !c,
      // With no database open, "This database" lists them all.
      here: !target || (!!c && target.connectionId === c.id && (s.database || c.database || null) === (target.database || c.database || null)),
    };
  });

  /** A name to suggest when a tab is saved for the first time. */
  const suggestedName = (t: EditorTab) => t.title || t.question.trim().slice(0, 60) || `${tabLabel(t).db ?? tabLabel(t).name} query`;

  const openSettings = (at?: { section: SettingsSection; aiAppsTab?: AiAppsTab }) => {
    setDialog({ kind: "settings", ...at });
    // Team AI's limits and usage may have changed since: ask again (only when signed in).
    if (settings.team) commands.teamAi().then(setTeamAi, () => {});
  };

  const finishSetup = () => {
    setSetupStep(null);
    commands.finishOnboarding().then(setSettings, () => {});
  };

  /**
   * Ends the setup with a tab that asks `question` and runs the answer: the tab the setup just
   * opened, while it's still empty, so the demo doesn't end up in two identical tabs.
   */
  const trySetupQuestion = (question: string) => {
    finishSetup();
    if (!target) return;
    const t = tabs.active;
    if (t && isPristine(t)) tabs.update(t.id, { question, autoAsk: true });
    else tabs.open(target, { question, autoAsk: true });
  };
  const openNewConnection = () => setDialog({ kind: "connection", config: newConnection() });

  // AI apps hand over a connection to finish or SQL to review; nothing runs or saves on its own.
  useHandoffs({
    connection: conn.connection,
    openConnectionDialog: (config, note) => setDialog({ kind: "connection", config, note, key: crypto.randomUUID() }),
    openTab: (t, init) => tabs.open(t, init),
    openScript,
    openTeamSignIn: (server, code) => setDialog({ kind: "teamSignIn", server, code, key: crypto.randomUUID() }),
    notify: setNotice,
  }, scripts.loaded);

  return (
    <div className={styles.workbench}>
      <TopBar
        active={active}
        databaseName={databaseName}
        schema={schema}
        onNewConnection={openNewConnection}
        pendingSuggestions={noteSuggestions.suggestions.length}
        // With suggested notes waiting, the gear opens them: Settings → AI apps → Suggestions.
        onOpenSettings={() => openSettings(noteSuggestions.suggestions.length > 0 ? { section: "mcp", aiAppsTab: "suggestions" } : undefined)}
      />

      {notice && (
        <div className={styles.banner}>
          <Alert
            tone="warning"
            actions={
              <Button size="sm" variant="ghost" onClick={() => setNotice(null)}>
                Dismiss
              </Button>
            }
          >
            {notice}
          </Alert>
        </div>
      )}

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
      {connActions.error && (
        <div className={styles.banner}>
          <Alert
            tone="danger"
            actions={
              <Button size="sm" variant="ghost" onClick={connActions.dismissError}>
                Dismiss
              </Button>
            }
          >
            {connActions.error}
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
              connected={targetConnected}
              onConnect={() => target && conn.connect(target)}
              onOpen={(id) => {
                const c = conn.connection(id);
                if (c && id !== active?.id) openConnection(c);
                else if (target) conn.connect(target);
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
              notes={notes}
              onEditNote={(note) => setDialog({ kind: "note", note })}
              onDeleteNote={(note) => void commands.deleteNote(note.id).then(() => setNotesVersion((v) => v + 1))}
              suggestedNotes={suggestionsHere.length}
              onReviewSuggestions={() => setDialog({ kind: "noteSuggestions" })}
              onTeach={teachHere ? () => setDialog({ kind: "teach" }) : undefined}
              onHideTeach={() => setTeachHidden(true)}
              onAgentSettings={(connection) => setDialog({ kind: "agentAccess", connection })}
              onTeamSignIn={settings.team ? undefined : () => setDialog({ kind: "teamSignIn", server: "", code: "", key: crypto.randomUUID() })}
            />
          }
          second={
            <div className={styles.editorArea}>
              <TabBar
                items={tabItems}
                activeId={tabs.active?.id ?? null}
                onSelect={tabs.activate}
                onClose={requestClose}
                menu={tabMenu}
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
                      connected={conn.isConnected(t.target.connectionId)}
                      production={!!conn.connection(t.target.connectionId)?.production}
                      onConnect={() => conn.connect(t.target)}
                      settings={settings}
                      modelNames={modelNames}
                      onChange={(patch) => tabs.update(t.id, patch)}
                      onOpenSettings={() => openSettings()}
                      skills={skills.skills}
                      onCreateSkill={() => setDialog({ kind: "settings", section: "skills", createSkill: true })}
                      onSave={() => requestSave(t)}
                      onOpenQuery={(sql, question) => tabs.open(t.target, { sql, question })}
                      onSaveAsScript={(sql, question) => {
                        const id = tabs.open(t.target, { sql, question });
                        setDialog({ kind: "saveScript", tabId: id, closeAfter: false });
                      }}
                      onAgentBusy={(busy) => setBusy(t.id, busy)}
                      onNotesChanged={() => setNotesVersion((v) => v + 1)}
                      onSettingsChange={setSettings}
                      onNotice={setNotice}
                      teamAi={teamAiFor(t.target.connectionId)}
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
        connected={targetConnected}
        onConnect={() => target && conn.connect(target)}
        modelNames={modelNames}
        aiReady={settings.aiReady}
        updates={updates}
        onFeedback={() => setDialog({ kind: "feedback" })}
      />

      {dialog?.kind === "connection" && (
        <ConnectionDialog
          // A new handoff replaces the form rather than keeping the old one's edits.
          key={dialog.key}
          initial={dialog.config}
          note={dialog.note}
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
            connActions.forget(id);
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
      {dialog?.kind === "closeTabs" && (() => {
        const closing = new Set(dialog.tabIds);
        const unsaved = tabs.tabs.filter((t) => closing.has(t.id) && hasUnsavedChanges(t));
        return (
          <CloseTabsDialog
            count={tabs.tabs.filter((t) => closing.has(t.id)).length}
            unsaved={unsaved.map((t) => tabItems.find((i) => i.id === t.id)?.label ?? "Untitled")}
            onCancel={() => setDialog(null)}
            onDiscard={() => {
              tabs.closeWhere((t) => closing.has(t.id));
              setDialog(null);
            }}
            onKeepUnsaved={() => {
              tabs.closeWhere((t) => closing.has(t.id) && !hasUnsavedChanges(t));
              setDialog(null);
            }}
          />
        );
      })()}
      {dialog?.kind === "agentAccess" && dialogAccess && (
        <AgentAccessDialog
          connectionName={dialog.connection.name}
          target={{ connectionId: dialog.connection.id, database: target?.connectionId === dialog.connection.id ? target.database : null }}
          provider={(() => {
            const ai = teamAiFor(dialog.connection.id);
            if (ai) return teamAiDestination(ai);
            const p = settings.providers.find((x) => x.id === settings.provider);
            return { label: p?.label ?? settings.provider, local: p?.local ?? false };
          })()}
          access={dialogAccess}
          onSave={async (a) => void (await saveAgentAccess(dialog.connection.id, a))}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === "note" && target && (
        <NoteDialog
          text={dialog.note?.text ?? ""}
          databaseName={databaseName ?? active?.name ?? "this database"}
          onSave={async (text) => {
            const base: DbNote = dialog.note ?? { id: "", connectionId: target.connectionId, database: target.database, text, updatedAt: 0 };
            await commands.saveNote({ ...base, text });
            setNotesVersion((v) => v + 1);
          }}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === "noteSuggestions" && (
        <Dialog
          title={`Notes suggested for ${databaseName ?? active?.name ?? "this database"}`}
          onClose={() => setDialog(null)}
          actions={<Button onClick={() => setDialog(null)}>Done</Button>}
        >
          <p className={styles.dialogLead}>
            AI apps suggest these from what they found. Accepted notes are saved like your own and go to the Ask box, agent
            mode and every AI app using SayQL.
          </p>
          <NoteSuggestions suggestions={suggestionsHere} onAccept={noteSuggestions.accept} onDismiss={noteSuggestions.dismiss} />
        </Dialog>
      )}
      {dialog?.kind === "teach" && active && (
        <Dialog
          title="Teach SayQL your database"
          onClose={() => setDialog(null)}
          actions={
            <>
              <Button
                variant="ghost"
                onClick={() => {
                  setTeachHidden(true);
                  setDialog(null);
                }}
              >
                Don&apos;t show again
              </Button>
              <Button onClick={() => setDialog(null)}>Done</Button>
            </>
          }
        >
          <TeachSayql
            heading={false}
            target={{ connectionId: active.id, name: active.name, database: target?.database ?? null }}
            onSetUp={() => openSettings({ section: "mcp", aiAppsTab: "access" })}
            onAudit={() => setDialog({ kind: "audit" })}
          />
        </Dialog>
      )}
      {dialog?.kind === "audit" && active && (
        <AuditDialog
          target={{ connectionId: active.id, name: active.name, database: target?.database ?? null }}
          onReview={() => setDialog({ kind: "noteSuggestions" })}
          onClose={() => setDialog(null)}
          onOpenAiSettings={() => openSettings({ section: "ai" })}
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
                onClick={() => {
                  setDialog(null);
                  void connActions.remove(dialog.connection.id);
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
        <SettingsPage
          settings={settings}
          connections={conn.connections}
          skills={skills}
          section={dialog.section}
          aiAppsTab={dialog.aiAppsTab}
          noteSuggestions={noteSuggestions}
          createSkill={dialog.createSkill}
          onShowSetup={() => {
            setDialog(null);
            setSetupStep("welcome");
          }}
          onClose={() => setDialog(null)}
          onSaved={setSettings}
          onTeamChange={setTeam}
        />
      )}
      {dialog?.kind === "teamSignIn" && (
        <TeamSignInDialog
          key={dialog.key}
          server={dialog.server}
          code={dialog.code}
          fromLink={!!dialog.server}
          onClose={() => setDialog(null)}
          onSignedIn={(account) => {
            setDialog(null);
            setTeam(account);
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
          onOpenSettings={() => openSettings()}
          onFinish={finishSetup}
        />
      )}
    </div>
  );
}
