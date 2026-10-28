import type { ConnectionConfig, NoteSuggestion } from "../../api";
import { NoteSuggestions, useNoteSuggestions } from "../agent";
import styles from "./ClaudeIntegration.module.css";

/** Notes AI apps suggested over MCP on any connection, waiting to be accepted or dismissed. */
export function SuggestedNotes({ connections }: { connections: ConnectionConfig[] }) {
  const { suggestions, accept, dismiss } = useNoteSuggestions();
  if (suggestions.length === 0) return null;
  const place = (s: NoteSuggestion) => {
    const name = connections.find((c) => c.id === s.connectionId)?.name ?? "A deleted connection";
    return s.database ? `${name} · ${s.database}` : name;
  };
  return (
    <div className={styles.access}>
      <h3 className={styles.heading}>
        Suggested notes <span className={styles.count}>{suggestions.length}</span>
      </h3>
      <p className={styles.intro}>
        AI apps can suggest a note about a database (a table that's no longer used, what a code means) with what they
        checked. Nothing is saved until you accept it; accepted notes go to the Ask box, agent mode and every AI app using
        SayQL. You also see a database's suggestions under Notes in the navigator.
      </p>
      <NoteSuggestions suggestions={suggestions} place={place} onAccept={accept} onDismiss={dismiss} />
    </div>
  );
}
