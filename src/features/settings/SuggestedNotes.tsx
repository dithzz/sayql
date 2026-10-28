import type { ConnectionConfig, NoteSuggestion } from "../../api";
import { NoteSuggestions, type NoteSuggestionsApi } from "../agent";
import styles from "./ClaudeIntegration.module.css";

export type SuggestedNotesProps = Pick<NoteSuggestionsApi, "suggestions" | "accept" | "dismiss"> & {
  connections: ConnectionConfig[];
};

/** Notes AI apps suggested over MCP on any connection, waiting to be accepted or dismissed. */
export function SuggestedNotes({ connections, suggestions, accept, dismiss }: SuggestedNotesProps) {
  const place = (s: NoteSuggestion) => {
    const name = connections.find((c) => c.id === s.connectionId)?.name ?? "A deleted connection";
    return s.database ? `${name} · ${s.database}` : name;
  };
  return (
    <div className={styles.activity}>
      <p className={styles.intro}>
        AI apps can suggest a note about a database (a table that's no longer used, what a code means) with what they
        checked. Nothing is saved until you accept it; accepted notes go to the Ask box, agent mode and every AI app using
        SayQL. You also see a database's suggestions under Notes in the navigator.
      </p>
      <NoteSuggestions suggestions={suggestions} place={place} onAccept={accept} onDismiss={dismiss} />
    </div>
  );
}
