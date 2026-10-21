import { useState } from "react";
import { PlugZap, Trash2 } from "lucide-react";
import { commands, type ConnectionConfig, type DatabaseKind } from "../../api";
import { Alert, Button, Checkbox, Dialog, Field, FieldRow, SegmentedControl, TextInput } from "../../components/ui";
import { errorText } from "../../lib/errors";
import { DATABASE_KINDS, KIND_OPTIONS } from "./kinds";

export function newConnection(): ConnectionConfig {
  return { id: "", name: "", kind: "postgres", host: "localhost", port: 5432, database: "", user: "", file: "", ssl: false };
}

function defaultName(c: ConnectionConfig): string {
  if (c.kind === "sqlite") return c.file.split("/").pop() || "SQLite";
  return `${c.database || DATABASE_KINDS[c.kind].label}@${c.host || "localhost"}`;
}

export interface ConnectionDialogProps {
  initial: ConnectionConfig;
  onClose: () => void;
  onSaved: (c: ConnectionConfig) => void;
  onDeleted: (id: string) => void;
}

export function ConnectionDialog({ initial, onClose, onSaved, onDeleted }: ConnectionDialogProps) {
  const isNew = !initial.id;
  const [c, setC] = useState(initial);
  // null = leave the password saved in the keychain unchanged.
  const [password, setPassword] = useState<string | null>(isNew ? "" : null);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [testing, setTesting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const set = (patch: Partial<ConnectionConfig>) => setC((prev) => ({ ...prev, ...patch }));

  const test = async () => {
    setTesting(true);
    setStatus(null);
    try {
      setStatus({ ok: true, text: await commands.testConnection(c, password) });
    } catch (e) {
      setStatus({ ok: false, text: errorText(e) });
    } finally {
      setTesting(false);
    }
  };

  const save = async () => {
    try {
      onSaved(await commands.saveConnection({ ...c, name: c.name.trim() || defaultName(c) }, password));
    } catch (e) {
      setStatus({ ok: false, text: errorText(e) });
    }
  };

  const remove = async () => {
    if (!confirmDelete) return setConfirmDelete(true);
    await commands.deleteConnection(c.id);
    onDeleted(c.id);
  };

  return (
    <Dialog
      title={isNew ? "New connection" : "Edit connection"}
      onClose={onClose}
      secondaryActions={
        !isNew && (
          <Button variant="dangerGhost" icon={Trash2} onClick={remove} title="Removes the saved connection; the database is not touched">
            {confirmDelete ? "Click again to delete" : "Delete"}
          </Button>
        )
      }
      actions={
        <>
          <Button icon={PlugZap} onClick={test} loading={testing}>
            Test
          </Button>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={save}>
            Save
          </Button>
        </>
      }
    >
      <Field label="Database type">
        {() => (
          <SegmentedControl<DatabaseKind>
            ariaLabel="Database type"
            value={c.kind}
            options={KIND_OPTIONS}
            onChange={(kind) => set({ kind, port: DATABASE_KINDS[kind].defaultPort })}
          />
        )}
      </Field>
      <Field label="Name" optional>
        {(id) => <TextInput id={id} value={c.name} placeholder={defaultName(c)} onChange={(e) => set({ name: e.target.value })} />}
      </Field>

      {c.kind === "sqlite" ? (
        <Field label="Database file">
          {(id) => (
            <TextInput id={id} mono value={c.file} placeholder="/path/to/database.sqlite" onChange={(e) => set({ file: e.target.value })} />
          )}
        </Field>
      ) : (
        <>
          <FieldRow>
            <Field label="Host" grow>
              {(id) => <TextInput id={id} value={c.host} onChange={(e) => set({ host: e.target.value })} />}
            </Field>
            <Field label="Port">
              {(id) => (
                <TextInput
                  id={id}
                  style={{ width: 90 }}
                  inputMode="numeric"
                  value={c.port ?? ""}
                  onChange={(e) => set({ port: e.target.value ? Number(e.target.value.replace(/\D/g, "")) : null })}
                />
              )}
            </Field>
          </FieldRow>
          <Field label="Database">
            {(id) => <TextInput id={id} value={c.database} onChange={(e) => set({ database: e.target.value })} />}
          </Field>
          <FieldRow>
            <Field label="User" grow>
              {(id) => <TextInput id={id} autoComplete="off" value={c.user} onChange={(e) => set({ user: e.target.value })} />}
            </Field>
            <Field label="Password" grow>
              {(id) => (
                <TextInput
                  id={id}
                  type="password"
                  value={password ?? ""}
                  placeholder={password === null ? "Saved in Keychain" : ""}
                  onChange={(e) => setPassword(e.target.value)}
                />
              )}
            </Field>
          </FieldRow>
          <Checkbox label="Require SSL" checked={c.ssl} onChange={(ssl) => set({ ssl })} />
        </>
      )}

      {status && <Alert tone={status.ok ? "success" : "danger"}>{status.text}</Alert>}
    </Dialog>
  );
}
