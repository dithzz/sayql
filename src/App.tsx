import { useCallback, useEffect, useRef, useState } from "react";
import Editor from "@monaco-editor/react";
import { monaco, setCompletionSchema } from "./monaco";
import { api, errorText, onAiEvents, type ConnConfig, type QueryResult, type Schema, type Settings, type Table } from "./api";
import { fastPath, previewSql } from "./fastpath";
import { Sidebar } from "./components/Sidebar";
import { ResultsGrid } from "./components/ResultsGrid";
import { ConfirmWriteDialog, ConnectionDialog, SettingsDialog, emptyConnection } from "./components/Dialogs";
import "./App.css";

/** Wait this long after the last keystroke before asking the AI. */
const DEBOUNCE_MS = 250;

type Gen =
  | { state: "idle" }
  | { state: "instant"; from: "rules" | "cache" }
  | { state: "writing" }
  | { state: "done"; firstTokenMs: number | null; totalMs: number }
  | { state: "error"; message: string; needsSettings?: boolean };

const DEMO_EXAMPLES = [
  "revenue by month this year",
  "top 10 customers by total spend",
  "products that have never been ordered",
  "average rating per category, best first",
  "orders that took more than 3 days to ship",
  "how many customers per country",
];

function cleanSql(s: string): string {
  return s
    .replace(/^\s*```(?:sql)?\s*\n?/i, "")
    .replace(/\n?```\s*$/, "")
    .trimStart();
}

export default function App() {
  const [connections, setConnections] = useState<ConnConfig[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [schema, setSchema] = useState<Schema | null>(null);
  const [schemaLoading, setSchemaLoading] = useState(false);
  const [banner, setBanner] = useState<string | null>(null);

  const [ask, setAsk] = useState("");
  const [sql, setSql] = useState("");
  const [gen, setGen] = useState<Gen>({ state: "idle" });
  const [result, setResult] = useState<QueryResult | null>(null);
  const [queryError, setQueryError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const [editing, setEditing] = useState<ConnConfig | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [confirmSql, setConfirmSql] = useState<string | null>(null);
  const [editorHeight, setEditorHeight] = useState(200);

  const reqId = useRef(0);
  const buffer = useRef("");
  const pendingQuestion = useRef("");
  const runWhenDone = useRef(false);
  const debounce = useRef<number | undefined>(undefined);
  const cache = useRef(new Map<string, string>());
  const askBox = useRef<HTMLTextAreaElement>(null);

  const cacheKey = (q: string) => `${activeId}|${settings?.model}|${q.trim().toLowerCase().replace(/\s+/g, " ")}`;

  useEffect(() => {
    api
      .init()
      .then(({ connections, settings }) => {
        setConnections(connections);
        setSettings(settings);
        if (connections[0]) openConnection(connections[0].id);
      })
      .catch((e) => setBanner(errorText(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------- running queries ----------

  const execute = useCallback(
    async (text: string, allowWrite = false) => {
      if (!activeId || !text.trim()) return;
      setRunning(true);
      setQueryError(null);
      try {
        setResult(await api.runQuery(activeId, text, allowWrite));
      } catch (e) {
        const msg = errorText(e);
        if (msg === "WRITE_CONFIRM") setConfirmSql(text);
        else setQueryError(msg);
      } finally {
        setRunning(false);
      }
    },
    [activeId],
  );

  // ---------- AI generation ----------

  const startGeneration = useCallback(
    (question: string, keyJustAdded = false) => {
      if (!activeId) return;
      if (!keyJustAdded && !settings?.hasApiKey) {
        setGen({ state: "error", message: "Add your Claude API key to turn English into SQL.", needsSettings: true });
        return;
      }
      const id = ++reqId.current;
      buffer.current = "";
      pendingQuestion.current = question;
      setGen({ state: "writing" });
      api.generateSql(activeId, question, id).catch((e) => {
        if (id === reqId.current) setGen({ state: "error", message: errorText(e) });
      });
    },
    [activeId, settings],
  );

  // Latest values for the event listeners, which are registered once.
  const latest = useRef({ execute, cacheKey });
  latest.current = { execute, cacheKey };
  const firstToken = useRef<number | null>(null);

  useEffect(() => {
    const un = onAiEvents({
      delta: (e) => {
        if (e.reqId !== reqId.current) return;
        if (e.firstTokenMs != null) firstToken.current = e.firstTokenMs;
        buffer.current += e.text;
        setSql(cleanSql(buffer.current));
      },
      reset: (id) => {
        if (id === reqId.current) buffer.current = "";
      },
      done: (e) => {
        if (e.reqId !== reqId.current) return;
        if (e.error) {
          setGen({ state: "error", message: e.error, needsSettings: e.error.includes("Settings") });
          runWhenDone.current = false;
          return;
        }
        const final = cleanSql(buffer.current).trimEnd();
        setSql(final);
        cache.current.set(latest.current.cacheKey(pendingQuestion.current), final);
        setGen({ state: "done", firstTokenMs: firstToken.current, totalMs: e.totalMs });
        firstToken.current = null;
        if (runWhenDone.current) {
          runWhenDone.current = false;
          latest.current.execute(final);
        }
      },
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const onAskChange = (text: string) => {
    setAsk(text);
    window.clearTimeout(debounce.current);
    runWhenDone.current = false;
    if (!text.trim()) {
      reqId.current++;
      api.cancelGeneration();
      setGen({ state: "idle" });
      return;
    }
    const instant = fastPath(text, schema);
    const cached = cache.current.get(cacheKey(text));
    if (instant || cached) {
      reqId.current++; // drop any in-flight answer for the previous text
      api.cancelGeneration();
      setSql((instant ?? cached)!);
      setGen({ state: "instant", from: instant ? "rules" : "cache" });
      return;
    }
    setGen({ state: "writing" });
    debounce.current = window.setTimeout(() => startGeneration(text), DEBOUNCE_MS);
  };

  const run = () => {
    if (gen.state === "writing") {
      // Run as soon as the SQL is ready; skip the debounce wait if it hasn't fired yet.
      runWhenDone.current = true;
      if (debounce.current !== undefined && pendingQuestion.current !== ask) {
        window.clearTimeout(debounce.current);
        debounce.current = undefined;
        startGeneration(ask);
      }
      return;
    }
    execute(sql);
  };
  const runRef = useRef(run);
  runRef.current = run;

  // ---------- connections ----------

  async function openConnection(id: string, refresh = false) {
    setActiveId(id);
    setSchemaLoading(true);
    setBanner(null);
    if (!refresh) {
      setSchema(null);
      setResult(null);
      setQueryError(null);
      setGen({ state: "idle" });
    }
    try {
      const s = refresh ? await api.refreshSchema(id) : await api.openConnection(id);
      setSchema(s);
      setCompletionSchema(s);
      askBox.current?.focus();
    } catch (e) {
      setBanner(`Could not connect: ${errorText(e)}`);
    } finally {
      setSchemaLoading(false);
    }
  }

  const previewTable = (t: Table) => {
    if (!schema) return;
    const q = previewSql(t, schema.dialect);
    setSql(q);
    setGen({ state: "idle" });
    execute(q);
  };

  const active = connections.find((c) => c.id === activeId) ?? null;
  const columnCount = schema?.tables.reduce((n, t) => n + t.columns.length, 0) ?? 0;
  const modelName = settings?.models.find((m) => m.id === settings.model)?.label.split(" — ")[0] ?? "";

  const startResize = (e: React.MouseEvent) => {
    const startY = e.clientY;
    const startH = editorHeight;
    const move = (ev: MouseEvent) => setEditorHeight(Math.max(90, Math.min(600, startH + ev.clientY - startY)));
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div className="app">
      <Sidebar
        connections={connections}
        activeId={activeId}
        schema={schema}
        loading={schemaLoading}
        onOpen={(id) => id !== activeId && openConnection(id)}
        onAdd={() => setEditing(emptyConnection())}
        onEdit={setEditing}
        onRefresh={() => activeId && openConnection(activeId, true)}
        onPreviewTable={previewTable}
        onSettings={() => setShowSettings(true)}
      />

      <main className="main">
        {banner && (
          <div className="banner">
            {banner}
            <button className="ghost small" onClick={() => setBanner(null)}>
              Dismiss
            </button>
          </div>
        )}

        {!active ? (
          <div className="empty-state">
            <h1>Ask your database anything</h1>
            <p>Add a connection to get started.</p>
            <button className="primary" onClick={() => setEditing(emptyConnection())}>
              New connection
            </button>
          </div>
        ) : (
          <>
            <section className="ask">
              <div className="ask-head">
                <span className="ask-label">Ask in plain English</span>
                <span className="ask-meta">
                  {schema
                    ? `${active.name} · knows ${schema.tables.length} tables, ${columnCount} columns`
                    : schemaLoading
                      ? `Reading ${active.name}…`
                      : active.name}
                </span>
              </div>
              <textarea
                ref={askBox}
                className="ask-input"
                rows={2}
                value={ask}
                disabled={!schema}
                placeholder="e.g. top 10 customers by total spend this year"
                onChange={(e) => onAskChange(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    run();
                  } else if (e.key === "Escape") {
                    onAskChange("");
                  }
                }}
              />
              <div className="ask-foot">
                <GenStatus gen={gen} modelName={modelName} onOpenSettings={() => setShowSettings(true)} />
                {!ask && active.id === "demo" && (
                  <div className="chips">
                    {DEMO_EXAMPLES.map((ex) => (
                      <button key={ex} className="chip" onClick={() => onAskChange(ex)}>
                        {ex}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            </section>

            <section className="sql">
              <div className="sql-bar">
                <span className="sql-label">SQL</span>
                <span className="hint">Edit freely · Enter in the Ask box or ⌘↵ here runs it</span>
                <span className="grow" />
                <button className="primary run" onClick={run} disabled={!schema || running || (!sql.trim() && gen.state !== "writing")}>
                  {running ? "Running…" : gen.state === "writing" ? "Run when ready" : "▶ Run"}
                </button>
              </div>
              <div style={{ height: editorHeight }}>
                <Editor
                  language="sql"
                  theme="askdb"
                  value={sql}
                  onChange={(v) => setSql(v ?? "")}
                  onMount={(editor) => {
                    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => runRef.current());
                  }}
                  options={{
                    minimap: { enabled: false },
                    fontSize: 13.5,
                    fontFamily: "'JetBrains Mono', 'SF Mono', Menlo, monospace",
                    lineNumbers: "on",
                    scrollBeyondLastLine: false,
                    wordWrap: "on",
                    automaticLayout: true,
                    padding: { top: 10 },
                    renderLineHighlight: "line",
                  }}
                />
              </div>
              <div className="resize" onMouseDown={startResize} />
            </section>

            <section className="output">
              {queryError ? (
                <div className="query-error">
                  <b>The database returned an error</b>
                  <pre>{queryError}</pre>
                  {ask && (
                    <span className="hint">Try rephrasing your question, or edit the SQL above.</span>
                  )}
                </div>
              ) : result ? (
                <ResultsGrid result={result} />
              ) : (
                <div className="results-empty">Results appear here.</div>
              )}
            </section>
          </>
        )}
      </main>

      {editing && (
        <ConnectionDialog
          initial={editing}
          onClose={() => setEditing(null)}
          onSaved={(c) => {
            setConnections((list) => (list.some((x) => x.id === c.id) ? list.map((x) => (x.id === c.id ? c : x)) : [...list, c]));
            setEditing(null);
            openConnection(c.id);
          }}
          onDeleted={(id) => {
            setConnections((list) => list.filter((x) => x.id !== id));
            setEditing(null);
            if (id === activeId) {
              setActiveId(null);
              setSchema(null);
              setCompletionSchema(null);
            }
          }}
        />
      )}
      {showSettings && settings && (
        <SettingsDialog
          settings={settings}
          onClose={() => setShowSettings(false)}
          onSaved={(s) => {
            setSettings(s);
            setShowSettings(false);
            if (gen.state === "error" && gen.needsSettings && s.hasApiKey && ask.trim()) startGeneration(ask, true);
          }}
        />
      )}
      {confirmSql && active && (
        <ConfirmWriteDialog
          sql={confirmSql}
          connName={active.name}
          onCancel={() => setConfirmSql(null)}
          onConfirm={() => {
            const q = confirmSql;
            setConfirmSql(null);
            execute(q, true);
          }}
        />
      )}
    </div>
  );
}

function GenStatus({ gen, modelName, onOpenSettings }: { gen: Gen; modelName: string; onOpenSettings: () => void }) {
  switch (gen.state) {
    case "idle":
      return <span className="status muted">SQL appears below as you type.</span>;
    case "instant":
      return (
        <span className="status instant">
          ⚡ Instant · {gen.from === "rules" ? "simple request, no AI needed" : "same question as before"}
        </span>
      );
    case "writing":
      return (
        <span className="status writing">
          <span className="spinner" /> Writing SQL…
        </span>
      );
    case "done":
      return (
        <span className="status ai">
          ✦ {modelName}
          {gen.firstTokenMs != null && ` · first SQL in ${gen.firstTokenMs} ms`} · done in {gen.totalMs} ms
        </span>
      );
    case "error":
      return (
        <span className="status error">
          {gen.message}
          {gen.needsSettings && (
            <button className="ghost small" onClick={onOpenSettings}>
              Open Settings
            </button>
          )}
        </span>
      );
  }
}
