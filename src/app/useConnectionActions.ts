import { useState } from "react";
import { commands } from "../api";
import type { ConnectionsState } from "../features/connections";
import type { useTabs } from "../features/tabs";
import { errorText } from "../lib/errors";

type Tabs = ReturnType<typeof useTabs>;

/** Deleting saved connections: the connection goes, and so do its tabs. */
export function useConnectionActions(conn: ConnectionsState, tabs: Tabs) {
  /** Why the last delete failed, for the banner. */
  const [error, setError] = useState<string | null>(null);

  /** Drops a connection that is already deleted (by the connection dialog) and closes its tabs. */
  const forget = (id: string) => {
    conn.remove(id);
    tabs.closeWhere((t) => t.target.connectionId === id);
  };

  /** Deletes the saved connection, then forgets it. On failure it stays, and `error` says why. */
  const remove = async (id: string): Promise<boolean> => {
    setError(null);
    try {
      await commands.deleteConnection(id);
    } catch (e) {
      setError(`Could not delete the connection: ${errorText(e)}`);
      return false;
    }
    forget(id);
    return true;
  };

  return { forget, remove, error, dismissError: () => setError(null) };
}
