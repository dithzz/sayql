import { useState } from "react";
import type { AgentAccess, AgentResults } from "../../api";
import { Alert, Button, Checkbox, Dialog, Field } from "../../components/ui";
import { cx } from "../../lib/cx";
import styles from "./AgentAccessDialog.module.css";

export interface AgentAccessDialogProps {
  connectionName: string;
  provider: { label: string; local: boolean };
  access: AgentAccess;
  /** Saves and closes; rejects with a message to show. */
  onSave: (access: AgentAccess) => Promise<void>;
  onClose: () => void;
}

const RESULTS: { value: AgentResults; label: string; detail: string }[] = [
  {
    value: "off",
    label: "Off",
    detail: "The AI sees your question, the schema and how many rows each query returned. No values from your tables.",
  },
  {
    value: "counts",
    label: "Counts and summaries only",
    detail: "Per result: the row count and, per column, its type, nulls, distinct values, and min and max for numbers and dates. No row values or group labels.",
  },
  {
    value: "rows",
    label: "Up to 20 rows per query, masked",
    detail: "Likely personal data (emails, phones, names, addresses, IDs) is replaced by tokens before it leaves; SayQL fills the real values back in on your screen.",
  },
];

/** Turns agent mode on for a connection and says exactly what it may send to the AI. */
export function AgentAccessDialog({ connectionName, provider, access, onSave, onClose }: AgentAccessDialogProps) {
  const first = !access.enabled;
  const [results, setResults] = useState<AgentResults>(access.results);
  // Ask before send starts on for cloud providers, off for ones on this computer.
  const [ask, setAsk] = useState(first ? !provider.local : access.askBeforeSend);
  const [neverSend, setNeverSend] = useState(access.neverSend.join("\n"));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = (enabled: boolean) => {
    setSaving(true);
    setError(null);
    const list = neverSend.split(/[\n,]/).map((c) => c.trim()).filter(Boolean);
    onSave({ enabled, results, askBeforeSend: ask, neverSend: list }).then(onClose, (e) => {
      setSaving(false);
      setError(String(e));
    });
  };

  const goesOut =
    results === "off"
      ? null
      : `${results === "rows" ? "Rows" : "Counts and column summaries"} from this database will go to ${provider.label}${provider.local ? ", which runs on this computer" : ". Their terms apply"}.`;

  return (
    <Dialog
      title={first ? `Turn on agent mode for ${connectionName}` : `Agent settings for ${connectionName}`}
      onClose={onClose}
      secondaryActions={
        !first && (
          <Button variant="dangerGhost" disabled={saving} onClick={() => save(false)}>
            Turn off agent mode
          </Button>
        )
      }
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={saving} onClick={() => save(true)}>
            {first ? "Turn on" : "Save"}
          </Button>
        </>
      }
    >
      <div className={styles.body}>
        <p className={styles.lead}>
          The agent answers in several read-only steps with <strong>{provider.label}</strong>. It never changes data: a write is
          shown as SQL for you to review. Choose what it may see of query results on this connection.
        </p>
        <div role="radiogroup" aria-label="Results sent to the AI" className={styles.options}>
          {RESULTS.map((r) => (
            <label key={r.value} className={cx(styles.option, results === r.value && styles.chosen)}>
              <input type="radio" name="results" value={r.value} checked={results === r.value} onChange={() => setResults(r.value)} />
              <span>
                <strong>{r.label}</strong>
                <span className={styles.detail}>{r.detail}</span>
              </span>
            </label>
          ))}
        </div>
        {goesOut && <Alert tone="warning">{goesOut}</Alert>}
        <Checkbox label="Ask before each send (shows exactly what would go, with Send and Skip)" checked={ask} onChange={setAsk} />
        <Field label="Never send these columns" hint="One per line, as column or table.column. Queries that use them are refused." optional>
          {(id) => (
            <textarea id={id} className={styles.textarea} rows={3} value={neverSend} onChange={(e) => setNeverSend(e.target.value)} placeholder={"patients.notes\ndiagnosis"} />
          )}
        </Field>
        <p className={styles.detail}>
          Masking is best effort: a free-text column can hold anything. For health or production data, keep results Off or
          Counts, or mark such columns never send.
        </p>
        {error && <Alert tone="danger">{error}</Alert>}
      </div>
    </Dialog>
  );
}
