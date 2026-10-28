import type { McpApp } from "../../api";
import { Badge, Button } from "../../components/ui";
import styles from "./ClaudeIntegration.module.css";

export function when(ms: number | null | undefined): string {
  return ms ? new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) : "Never";
}

/** The AI apps that used SayQL over MCP (or were blocked), each allowed or blocked. `null` while loading. */
export function AppsList({ apps, onToggle }: { apps: McpApp[] | null; onToggle: (app: McpApp) => void }) {
  return (
    <section className={styles.group} aria-labelledby="ai-apps-heading">
      <h3 id="ai-apps-heading" className={styles.heading}>
        Apps
      </h3>
      <p className={styles.intro}>
        Block an app to refuse it everything. Apps give their own name when they connect, so a block stops an app, not
        software that pretends to be it.
      </p>
      {apps === null ? null : apps.length === 0 ? (
        <p className={styles.empty}>No AI app has connected yet. Connect one below; it shows up here after its first request.</p>
      ) : (
        <ul className={styles.list} aria-label="AI apps">
          {apps.map((a) => (
            <li key={a.name} className={styles.row}>
              <div className={styles.name}>
                <span className={styles.appName}>
                  {a.name}
                  <Badge tone={a.blocked ? "danger" : "success"}>{a.blocked ? "Blocked" : "Allowed"}</Badge>
                </span>
                <span className={styles.detail}>
                  {a.queries} {a.queries === 1 ? "request" : "requests"} in the log · last seen {when(a.lastUsed)}
                </span>
              </div>
              <Button
                size="sm"
                variant={a.blocked ? "secondary" : "dangerGhost"}
                aria-label={`${a.blocked ? "Allow" : "Block"} ${a.name}`}
                onClick={() => onToggle(a)}
              >
                {a.blocked ? "Allow" : "Block"}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
