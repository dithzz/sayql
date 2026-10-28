import { useCallback, useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import { commands, type McpActivity, type McpApp } from "../../api";
import { Alert, Button } from "../../components/ui";
import { cx } from "../../lib/cx";
import { errorText } from "../../lib/errors";
import styles from "./ClaudeIntegration.module.css";

const STATUS: Record<McpActivity["status"], string> = { ok: "OK", refused: "Refused", error: "Error" };

function when(ms: number | null | undefined): string {
  return ms ? new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) : "Never";
}

/** Which AI apps used SayQL over MCP, what they ran, and blocking an app. */
export function AgentActivity() {
  const [apps, setApps] = useState<McpApp[]>([]);
  const [log, setLog] = useState<McpActivity[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    Promise.all([commands.mcpApps(), commands.mcpActivity(50)]).then(
      ([a, l]) => {
        setApps(a);
        setLog(l);
      },
      (e) => setError(errorText(e)),
    );
  }, []);
  useEffect(load, [load]);

  const toggle = (app: McpApp) => commands.setMcpAppBlocked(app.name, !app.blocked).then(setApps, (e) => setError(errorText(e)));
  const clear = () => {
    if (!window.confirm("Delete the activity log? Blocked apps stay blocked.")) return;
    commands.clearMcpActivity().then(load, (e) => setError(errorText(e)));
  };

  return (
    <div className={styles.access}>
      <div className={styles.headRow}>
        <h3 className={styles.heading}>Agent activity</h3>
        <Button size="sm" variant="ghost" icon={RefreshCw} onClick={load}>
          Refresh
        </Button>
      </div>
      <p className={styles.intro}>
        What AI apps ran through SayQL on this computer: the SQL and how many rows, never the rows. Block an app to refuse
        it everything. Apps give their own name when they connect, so a block stops an app, not software that pretends to be
        it.
      </p>
      {apps.length > 0 && (
        <ul className={styles.list}>
          {apps.map((a) => (
            <li key={a.name} className={styles.row}>
              <div className={styles.name}>
                <span>
                  {a.name}
                  {a.blocked && <span className={styles.blocked}> · Blocked</span>}
                </span>
                <span className={styles.detail}>
                  {a.queries} {a.queries === 1 ? "query" : "queries"} · last used {when(a.lastUsed)}
                </span>
              </div>
              <Button size="sm" variant={a.blocked ? "secondary" : "dangerGhost"} onClick={() => toggle(a)}>
                {a.blocked ? "Allow" : "Block"}
              </Button>
            </li>
          ))}
        </ul>
      )}
      {log.length === 0 ? (
        <p className={styles.detail}>Nothing yet. Queries from Claude, Cursor and other MCP apps show up here.</p>
      ) : (
        <>
          <div className={styles.tableWrap}>
            <table className={styles.table}>
              <thead>
                <tr>
                  <th>When</th>
                  <th>App</th>
                  <th>Connection</th>
                  <th>Result</th>
                  <th>SQL</th>
                </tr>
              </thead>
              <tbody>
                {log.map((e, i) => (
                  <tr key={`${e.at}-${i}`} className={cx(e.status !== "ok" && styles.notOk)}>
                    <td>{when(e.at)}</td>
                    <td>{e.app}</td>
                    <td>{e.connection || "—"}</td>
                    <td>
                      {STATUS[e.status]}
                      {e.status === "ok" && e.rows != null && ` · ${e.rows} rows`}
                      {e.detail && <span className={styles.detail}> · {e.detail}</span>}
                    </td>
                    <td className={styles.sql} title={e.sql}>
                      {e.tool === "export_csv" ? `CSV export: ${e.sql}` : e.sql || e.tool}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div>
            <Button size="sm" variant="dangerGhost" onClick={clear}>
              Clear the log
            </Button>
          </div>
        </>
      )}
      {error && <Alert tone="danger">{error}</Alert>}
    </div>
  );
}
