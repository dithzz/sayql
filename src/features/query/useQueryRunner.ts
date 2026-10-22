import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Target } from "../../api";
import { useLatest } from "../../hooks/useLatest";
import { splitForRun } from "../../lib/sqlStatements";
import { targetKey } from "../connections";
import { runStatements, type Outcome } from "./runStatements";

export interface QueryRunner {
  /** One per statement of the last run, as far as it got. */
  outcomes: Outcome[];
  /** How many statements the last run had. */
  total: number;
  /** The database error that stopped the last run, if any. */
  error: string | null;
  running: boolean;
  /** A data-changing statement waiting for the user's confirmation. */
  pendingWrite: string | null;
  run: (sql: string) => void;
  confirmWrite: () => void;
  cancelWrite: () => void;
}

/**
 * Runs SQL on the tab's database, one statement at a time; each statement that changes
 * data waits for confirmation. `onFailure` hears about the error of a single-statement
 * run, e.g. to offer an AI fix (a script is the person's own, so it's left to them).
 */
export function useQueryRunner(target: Target | null, onFailure?: (sql: string, error: string) => void): QueryRunner {
  const failure = useLatest(onFailure);
  const [outcomes, setOutcomes] = useState<Outcome[]>([]);
  const [total, setTotal] = useState(0);
  const [running, setRunning] = useState(false);
  const [pendingWrite, setPendingWrite] = useState<string | null>(null);
  const answer = useRef<((ok: boolean) => void) | null>(null);
  /** Bumped by each run and database change; a stale run's progress is dropped. */
  const runId = useRef(0);

  const settle = (ok: boolean) => {
    answer.current?.(ok);
    answer.current = null;
    setPendingWrite(null);
  };

  const key = targetKey(target);
  useEffect(() => {
    runId.current++;
    settle(false);
    setOutcomes([]);
    setTotal(0);
    setRunning(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const run = useCallback(
    async (sql: string) => {
      const statements = splitForRun(sql);
      if (!target || statements.length === 0) return;
      const id = ++runId.current;
      settle(false);
      setOutcomes([]);
      setTotal(statements.length);
      setRunning(true);
      const out = await runStatements(
        statements,
        (s, allowWrite) => commands.runQuery(target, s, allowWrite),
        (s) =>
          new Promise<boolean>((resolve) => {
            if (id !== runId.current) return resolve(false);
            answer.current = resolve;
            setPendingWrite(s);
          }),
        (o) => id === runId.current && setOutcomes(o),
      );
      if (id !== runId.current) return;
      setRunning(false);
      const last = out[out.length - 1];
      if (statements.length === 1 && last?.error) failure.current?.(last.sql, last.error);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [target, failure],
  );

  return {
    outcomes,
    total,
    error: outcomes.find((o) => o.error)?.error ?? null,
    running,
    pendingWrite,
    run,
    confirmWrite: () => settle(true),
    cancelWrite: () => settle(false),
  };
}
