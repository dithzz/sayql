import { useEffect, useState } from "react";
import { commands, type AgentAccess, type AppResults, type ConnectionConfig } from "../../api";
import { Alert, Select } from "../../components/ui";
import { errorText } from "../../lib/errors";
import { saveAgentAccess } from "../agent";
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

/** Per saved connection: what AI apps over MCP may see of query results. */
export function AppAccessList({ connections }: { connections: ConnectionConfig[] }) {
  // MCP serves the connections saved on this computer, not a team's.
  const local = connections.filter((c) => !c.id.startsWith("team:"));
  const [access, setAccess] = useState<Record<string, AgentAccess>>({});
  const [error, setError] = useState<string | null>(null);

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

  const choose = (id: string, apps: AppResults) => {
    const { team: _team, ...current } = access[id];
    setError(null);
    saveAgentAccess(id, { ...current, apps }).then(
      (saved) => setAccess((all) => ({ ...all, [id]: saved })),
      (e) => setError(errorText(e)),
    );
  };

  if (local.length === 0) return null;
  return (
    <div className={styles.access}>
      <h3 className={styles.heading}>What AI apps may see</h3>
      <p className={styles.intro}>
        Per connection, choose what an AI app gets back from a query. Columns you mark never send in a connection&apos;s
        agent settings are always kept out.
      </p>
      <ul className={styles.list}>
        {local.map((c) => {
          const a = access[c.id];
          const chosen = APP_RESULTS.find((r) => r.value === a?.apps);
          return (
            <li key={c.id} className={styles.row}>
              <div className={styles.name}>
                <span>{c.name}</span>
                {chosen && <span className={styles.detail}>{chosen.detail}</span>}
              </div>
              <Select
                aria-label={`What AI apps may see on ${c.name}`}
                value={a?.apps ?? "counts"}
                disabled={!a}
                options={APP_RESULTS.map((r) => ({ value: r.value, label: r.label }))}
                onChange={(v) => choose(c.id, v)}
              />
            </li>
          );
        })}
      </ul>
      {error && <Alert tone="danger">{error}</Alert>}
    </div>
  );
}
