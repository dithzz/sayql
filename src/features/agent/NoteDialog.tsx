import { useState } from "react";
import { Alert, Button, Dialog, Field } from "../../components/ui";
import styles from "./AgentAccessDialog.module.css";

export interface NoteDialogProps {
  /** The note's text when editing; empty for a new note. */
  text: string;
  databaseName: string;
  /** Saves and closes; rejects with a message to show (a note that holds a value is refused). */
  onSave: (text: string) => Promise<void>;
  onClose: () => void;
}

/** Adds or edits a database note: a fact about tables, columns or codes, never values from rows. */
export function NoteDialog({ text: initial, databaseName, onSave, onClose }: NoteDialogProps) {
  const [text, setText] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  return (
    <Dialog
      title={initial ? "Edit database note" : `Add a note to ${databaseName}`}
      size="sm"
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!text.trim()} onClick={() => onSave(text).then(onClose, (e) => setError(String(e)))}>
            Save
          </Button>
        </>
      }
    >
      <div className={styles.body}>
        <Field label="Note" hint="A fact that helps answer questions here, e.g. “the owner of a tenant is memberships.role = 'owner'”. The Ask box, agent mode and MCP all use it.">
          {(id) => <textarea id={id} className={styles.textarea} rows={3} maxLength={300} value={text} onChange={(e) => setText(e.target.value)} autoFocus />}
        </Field>
        {error && <Alert tone="danger">{error}</Alert>}
      </div>
    </Dialog>
  );
}
