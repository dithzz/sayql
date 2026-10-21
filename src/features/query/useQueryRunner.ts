import { useCallback, useEffect, useState } from "react";
import { commands, WRITE_CONFIRM, type QueryResult } from "../../api";
import { errorText } from "../../lib/errors";

export interface QueryRunner {
  result: QueryResult | null;
  error: string | null;
  running: boolean;
  /** A data-changing statement waiting for the user's confirmation. */
  pendingWrite: string | null;
  run: (sql: string) => void;
  confirmWrite: () => void;
  cancelWrite: () => void;
}

/** Runs SQL on the open connection. Statements that change data wait for confirmation. */
export function useQueryRunner(connectionId: string | null): QueryRunner {
  const [result, setResult] = useState<QueryResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [pendingWrite, setPendingWrite] = useState<string | null>(null);

  useEffect(() => {
    setResult(null);
    setError(null);
    setPendingWrite(null);
  }, [connectionId]);

  const execute = useCallback(
    async (sql: string, allowWrite: boolean) => {
      if (!connectionId || !sql.trim()) return;
      setRunning(true);
      setError(null);
      try {
        setResult(await commands.runQuery(connectionId, sql, allowWrite));
      } catch (e) {
        const message = errorText(e);
        if (message === WRITE_CONFIRM) setPendingWrite(sql);
        else setError(message);
      } finally {
        setRunning(false);
      }
    },
    [connectionId],
  );

  return {
    result,
    error,
    running,
    pendingWrite,
    run: useCallback((sql: string) => execute(sql, false), [execute]),
    confirmWrite: () => {
      if (pendingWrite) execute(pendingWrite, true);
      setPendingWrite(null);
    },
    cancelWrite: () => setPendingWrite(null),
  };
}
