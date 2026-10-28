import { useEffect, useState } from "react";
import { Pencil } from "lucide-react";
import { commands, type AgentAccess, type AppResults, type ConnectionConfig } from "../../api";
import { Alert, Badge, Button, Select } from "../../components/ui";
import { cx } from "../../lib/cx";
import { errorText } from "../../lib/errors";
import { AgentAccessDialog, NeverSendLabel, saveAgentAccess } from "../agent";
import { DATABASE_KINDS } from "../connections";
import styles from "./ClaudeIntegration.module.css";

export const APP_RESULTS: { value: AppResults; label: string; detail: string }[] = [
  { value: "off", label: "Nothing", detail: "Only how many rows a query returned, and its column names." },
  {
    value: "counts",
    label: "Counts and summaries",
    detail: "Row counts and, per column, its type, nulls, distinct values, and min and max for numbers and dates. No row values.",
  },
  {
    value: "masked",
    label: "Rows, masked",
    detail: "Rows, with likely personal data (emails, phones, names, IDs) replaced by tokens. Best effort: free text can hold anything.",
  },
  { value: "all", label: "All rows", detail: "Rows as they are, and CSV export. For data you're happy to share with that app." },
];

/** Per saved connection: what AI apps over MCP get back from a query, and the columns never sent. */
export function AppAccessList({ connections, provider }: { connections: ConnectionConfig[]; provider: { label: string; local: boolean } }) {
  // MCP serves the connections saved on this computer, not a team's.
  const local = connections.filter((c) => !c.id.startsWith("team:"));
  const [access, setAccess] = useState<Record<string, AgentAccess>>({});
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<ConnectionConfig | null>(null);

  useEffect(() => {
    let live = true;
    Promise.all(local.map((c) => commands.agentAccess(c.id).then((a) => [c.id, a] as const))).then(
      (pairs) => live && setAccess(Object.fromEntries(pairs)),
      (e) => live && setError(errorText(e)),
    );
    return () => {
      live = false;
    };
    // The list of ids, not the array, decides what to load.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [local.map((c) => c.id).join("\n")]);

  const save = (id: string, next: AgentAccess) =>
    saveAgentAccess(id, next).then((saved) => setAccess((all) => ({ ...all, [id]: saved })));

  const choose = (id: string, apps: AppResults) => {
    const { team: _team, ...current } = access[id];
    setError(null);
    save(id, { ...current, apps }).catch((e) => setError(errorText(e)));
  };

  return (
    <section className={styles.group} aria-labelledby="ai-apps-connections">
      <h3 id="ai-apps-connections" className={styles.heading}>
        Connections
      </h3>
      <p className={styles.intro}>
        Per connection, choose what an AI app gets back from a query. Never-send columns are always kept out: queries
        that use them are refused. They apply to agent mode too.
      </p>
      <dl className={styles.legend} aria-label="What each choice sends">
        {APP_RESULTS.map((r) => (
          <div key={r.value} className={styles.legendItem}>
            <dt>{r.label}</dt>
            <dd>{r.detail}</dd>
          </div>
        ))}
      </dl>
      {local.length === 0 ? (
        <p className={styles.empty}>No saved connections yet. AI apps can use the connections saved on this computer.</p>
      ) : (
        <div className={styles.tableWrap}>
          <table className={cx(styles.table, styles.accessTable)}>
            <colgroup>
              <col />
              <col className={styles.colChoice} />
              <col className={styles.colNever} />
            </colgroup>
            <thead>
              <tr>
                <th>Connection</th>
                <th>What AI apps get back</th>
                <th>Never send</th>
              </tr>
            </thead>
            <tbody>
              {local.map((c) => {
                const a = access[c.id];
                return (
                  <tr key={c.id}>
                    <td>
                      <div className={styles.connName}>
                        <span className={styles.ellipsis} title={c.name}>
                          {c.name}
                        </span>
                        <span className={styles.detail}>{DATABASE_KINDS[c.kind]?.label ?? c.kind}</span>
                      </div>
                    </td>
                    <td>
                      <Select
                        aria-label={`What AI apps may see on ${c.name}`}
                        className={styles.choice}
                        value={a?.apps ?? "counts"}
                        disabled={!a}
                        options={APP_RESULTS.map((r) => ({ value: r.value, label: r.label }))}
                        onChange={(v) => choose(c.id, v)}
                      />
                    </td>
                    <td>
                      <div className={styles.chips}>
                        {a && a.neverSend.length === 0 && <span className={styles.detail}>None</span>}
                        {a?.neverSend.map((col) => (
                          <Badge key={col} title={col.includes(".") ? undefined : `${col}, in every table`}>
                            <span className={styles.chip}>
                              <NeverSendLabel entry={col} />
                            </span>
                          </Badge>
                        ))}
                        <Button
                          size="sm"
                          variant="ghost"
                          icon={Pencil}
                          disabled={!a}
                          aria-label={`Edit never-send columns on ${c.name}`}
                          onClick={() => setEditing(c)}
                        >
                          Edit
                        </Button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {error && <Alert tone="danger">{error}</Alert>}
      {editing && access[editing.id] && (
        <AgentAccessDialog
          neverSendOnly
          connectionName={editing.name}
          target={{ connectionId: editing.id, database: null }}
          provider={provider}
          access={access[editing.id]}
          onSave={async (next) => void (await save(editing.id, next))}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  );
}
