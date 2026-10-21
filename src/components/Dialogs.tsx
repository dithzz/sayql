import { useState, type ReactNode } from "react";
import { api, errorText, type ConnConfig, type Kind, type Settings } from "../api";

function Modal({ title, children, onClose }: { title: string; children: ReactNode; onClose: () => void }) {
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}

const DEFAULT_PORT: Record<Kind, number | null> = { postgres: 5432, mysql: 3306, sqlite: null };

export function emptyConnection(): ConnConfig {
  return { id: "", name: "", kind: "postgres", host: "localhost", port: 5432, database: "", user: "", file: "", ssl: false };
}

export function ConnectionDialog(props: {
  initial: ConnConfig;
  onClose: () => void;
  onSaved: (c: ConnConfig) => void;
  onDeleted: (id: string) => void;
}) {
  const [c, setC] = useState<ConnConfig>(props.initial);
  // null = keep the stored password unchanged.
  const [password, setPassword] = useState<string | null>(props.initial.id ? null : "");
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const isNew = !props.initial.id;
  const set = (patch: Partial<ConnConfig>) => setC((prev) => ({ ...prev, ...patch }));

  const test = async () => {
    setBusy(true);
    setStatus(null);
    try {
      setStatus({ ok: true, text: await api.testConnection(c, password) });
    } catch (e) {
      setStatus({ ok: false, text: errorText(e) });
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    const name = c.name.trim() || (c.kind === "sqlite" ? c.file.split("/").pop() || "SQLite" : `${c.database || c.kind}@${c.host}`);
    try {
      props.onSaved(await api.saveConnection({ ...c, name }, password));
    } catch (e) {
      setStatus({ ok: false, text: errorText(e) });
    }
  };

  const remove = async () => {
    if (!confirmDelete) {
      setConfirmDelete(true);
      return;
    }
    await api.deleteConnection(c.id);
    props.onDeleted(c.id);
  };

  return (
    <Modal title={isNew ? "New connection" : "Edit connection"} onClose={props.onClose}>
      <div className="form">
        <label>
          Database type
          <div className="segmented">
            {(["postgres", "mysql", "sqlite"] as Kind[]).map((k) => (
              <button
                key={k}
                className={c.kind === k ? "on" : ""}
                onClick={() => set({ kind: k, port: DEFAULT_PORT[k] })}
              >
                {k === "postgres" ? "PostgreSQL" : k === "mysql" ? "MySQL / MariaDB" : "SQLite"}
              </button>
            ))}
          </div>
        </label>
        <label>
          Name
          <input value={c.name} placeholder="e.g. Production analytics" onChange={(e) => set({ name: e.target.value })} />
        </label>
        {c.kind === "sqlite" ? (
          <label>
            Database file
            <input value={c.file} placeholder="/path/to/database.sqlite" onChange={(e) => set({ file: e.target.value })} />
          </label>
        ) : (
          <>
            <div className="row">
              <label className="grow">
                Host
                <input value={c.host} onChange={(e) => set({ host: e.target.value })} />
              </label>
              <label className="port">
                Port
                <input
                  value={c.port ?? ""}
                  inputMode="numeric"
                  onChange={(e) => set({ port: e.target.value ? Number(e.target.value.replace(/\D/g, "")) : null })}
                />
              </label>
            </div>
            <label>
              Database
              <input value={c.database} onChange={(e) => set({ database: e.target.value })} />
            </label>
            <div className="row">
              <label className="grow">
                User
                <input value={c.user} autoComplete="off" onChange={(e) => set({ user: e.target.value })} />
              </label>
              <label className="grow">
                Password
                <input
                  type="password"
                  value={password ?? ""}
                  placeholder={password === null ? "•••• saved in Keychain" : ""}
                  onChange={(e) => setPassword(e.target.value)}
                />
              </label>
            </div>
            <label className="check">
              <input type="checkbox" checked={c.ssl} onChange={(e) => set({ ssl: e.target.checked })} /> Require SSL
            </label>
          </>
        )}
        {status && <div className={status.ok ? "note ok" : "note err"}>{status.text}</div>}
      </div>
      <div className="modal-actions">
        {!isNew && (
          <button className="danger-ghost" onClick={remove} title="Removes the saved connection; the database is not touched">
            {confirmDelete ? "Click again to delete" : "Delete"}
          </button>
        )}
        <span className="grow" />
        <button className="ghost" onClick={test} disabled={busy}>
          {busy ? "Testing…" : "Test"}
        </button>
        <button className="ghost" onClick={props.onClose}>
          Cancel
        </button>
        <button className="primary" onClick={save}>
          Save
        </button>
      </div>
    </Modal>
  );
}

export function SettingsDialog(props: { settings: Settings; onClose: () => void; onSaved: (s: Settings) => void }) {
  const [model, setModel] = useState(props.settings.model);
  const [key, setKey] = useState("");
  const [workspaceId, setWorkspaceId] = useState(props.settings.workspaceId);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    try {
      props.onSaved(await api.saveSettings(model, key.trim() ? key.trim() : null, workspaceId));
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <Modal title="Settings" onClose={props.onClose}>
      <div className="form">
        <label>
          Claude API key
          <input
            type="password"
            value={key}
            placeholder={props.settings.hasApiKey ? "•••• saved in Keychain — paste to replace" : "sk-ant-…"}
            onChange={(e) => setKey(e.target.value)}
          />
          <span className="hint">
            Stored in your system keychain. Only your question and the database schema (table and column names) are
            sent to Claude — never the data in your tables.
          </span>
        </label>
        <label>
          Workspace ID <span className="hint">(optional)</span>
          <input
            value={workspaceId}
            placeholder="An ID code, not the workspace name — only needed for some keys"
            onChange={(e) => setWorkspaceId(e.target.value)}
          />
          <span className="hint">
            Some API keys work across workspaces and need to know which one to use. Find the ID in the Claude Console
            under your workspace’s settings.
          </span>
        </label>
        <label>
          Model
          <select value={model} onChange={(e) => setModel(e.target.value)}>
            {props.settings.models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
          <span className="hint">Haiku writes SQL fastest. Opus is the most accurate on complex questions.</span>
        </label>
        {error && <div className="note err">{error}</div>}
      </div>
      <div className="modal-actions">
        <span className="grow" />
        <button className="ghost" onClick={props.onClose}>
          Cancel
        </button>
        <button className="primary" onClick={save}>
          Save
        </button>
      </div>
    </Modal>
  );
}

export function ConfirmWriteDialog(props: { sql: string; connName: string; onCancel: () => void; onConfirm: () => void }) {
  return (
    <Modal title="This query changes data" onClose={props.onCancel}>
      <p className="confirm-text">
        It will modify <b>{props.connName}</b>. Changes like this can’t be undone from AskDB. Review it before running:
      </p>
      <pre className="confirm-sql">{props.sql}</pre>
      <div className="modal-actions">
        <span className="grow" />
        <button className="ghost" onClick={props.onCancel} autoFocus>
          Cancel
        </button>
        <button className="danger" onClick={props.onConfirm}>
          Run it
        </button>
      </div>
    </Modal>
  );
}
