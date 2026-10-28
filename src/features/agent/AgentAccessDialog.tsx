import { useState } from "react";
import type { AgentAccess, AgentResults, Target } from "../../api";
import { Alert, Button, Checkbox, Dialog } from "../../components/ui";
import { cx } from "../../lib/cx";
import styles from "./AgentAccessDialog.module.css";
import { NeverSendPicker } from "./NeverSendPicker";

export interface AgentAccessDialogProps {
  connectionName: string;
  /** The database whose tables the never-send picker shows (access itself is per connection). */
  target: Target;
  provider: { label: string; local: boolean };
  access: AgentAccess;
  /** Saves and closes; rejects with a message to show. */
  onSave: (access: AgentAccess) => Promise<void>;
  onClose: () => void;
  /**
   * Only the never-send columns (Settings → AI apps): agent mode stays as it is, on or off.
   * Never-send applies to agent mode and to AI apps over MCP alike.
   */
  neverSendOnly?: boolean;
}

const RANK: Record<AgentResults, number> = { off: 0, counts: 1, rows: 2 };

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
export function AgentAccessDialog({ connectionName, target, provider, access, onSave, onClose, neverSendOnly = false }: AgentAccessDialogProps) {
  const first = !access.enabled && !neverSendOnly;
  const [results, setResults] = useState<AgentResults>(access.results);
  // Ask before send starts on for cloud providers, off for ones on this computer.
  const [ask, setAsk] = useState(first ? !provider.local : access.askBeforeSend);
  // On a team connection the admin's rules win: results no higher than theirs, their columns always kept.
  const team = access.team ?? null;
  const adminColumn = (c: string) => !!team?.neverSend.some((t) => t.toLowerCase() === c.toLowerCase());
  const allowed = (r: AgentResults) => !team || RANK[r] <= RANK[team.maxResults];
  const [neverSend, setNeverSend] = useState(() => access.neverSend.filter((c) => !adminColumn(c)));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = (enabled: boolean) => {
    setSaving(true);
    setError(null);
    onSave({ enabled, results, apps: access.apps, askBeforeSend: ask, neverSend }).then(onClose, (e) => {
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
      title={
        neverSendOnly ? `Never send on ${connectionName}` : first ? `Turn on agent mode for ${connectionName}` : `Agent settings for ${connectionName}`
      }
      onClose={onClose}
      size="lg"
      secondaryActions={
        !first &&
        !neverSendOnly && (
          <Button variant="dangerGhost" disabled={saving} onClick={() => save(false)}>
            Turn off agent mode
          </Button>
        )
      }
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={saving} onClick={() => save(neverSendOnly ? access.enabled : true)}>
            {first ? "Turn on" : "Save"}
          </Button>
        </>
      }
    >
      <div className={styles.body}>
        {neverSendOnly ? (
          <p className={styles.lead}>
            Columns picked here never leave SayQL: queries that use them are refused, for AI apps over MCP and for agent
            mode. Agent mode stays {access.enabled ? "on" : "off"} for this connection.
          </p>
        ) : (
          <>
            <p className={styles.lead}>
              The agent answers in several read-only steps with <strong>{provider.label}</strong>. It never changes data: a write is
              shown as SQL for you to review. Choose what it may see of query results on this connection.
            </p>
            <div role="radiogroup" aria-label="Results sent to the AI" className={styles.options}>
              {RESULTS.map((r) => (
                <label key={r.value} className={cx(styles.option, results === r.value && styles.chosen, !allowed(r.value) && styles.locked)}>
                  <input
                    type="radio"
                    name="results"
                    value={r.value}
                    checked={results === r.value}
                    disabled={!allowed(r.value)}
                    onChange={() => setResults(r.value)}
                  />
                  <span>
                    <strong>
                      {r.label}
                      {!allowed(r.value) && <span className={styles.admin}> · Set by your team admin</span>}
                    </strong>
                    <span className={styles.detail}>{r.detail}</span>
                  </span>
                </label>
              ))}
            </div>
            {goesOut && <Alert tone="warning">{goesOut}</Alert>}
            <Checkbox label="Ask before each send (shows exactly what would go, with Send and Skip)" checked={ask} onChange={setAsk} />
          </>
        )}
        {team && team.neverSend.length > 0 && (
          <p className={styles.detail}>
            Set by your team admin, never sent: <span className={styles.adminColumns}>{team.neverSend.join(", ")}</span>. Your team's
            server also leaves them out of results.
          </p>
        )}
        <NeverSendPicker
          target={target}
          label={team?.neverSend.length ? "Also never send these columns" : "Never send these columns"}
          value={neverSend}
          onChange={setNeverSend}
          locked={team?.neverSend ?? []}
          autoFocus={neverSendOnly}
        />
        {!neverSendOnly && (
          <p className={styles.detail}>
            Masking is best effort: a free-text column can hold anything. For health or production data, keep results Off or
            Counts, or mark such columns never send.
          </p>
        )}
        {error && <Alert tone="danger">{error}</Alert>}
      </div>
    </Dialog>
  );
}
