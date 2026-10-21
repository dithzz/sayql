import { useCallback, useEffect, useRef, useState } from "react";
import { commands, subscribeToAi, type Schema } from "../../api";
import { useLatest } from "../../hooks/useLatest";
import { errorText } from "../../lib/errors";
import { fastPath } from "./fastPath";

/** Wait this long after the last keystroke before asking the AI. */
const DEBOUNCE_MS = 250;

export type GenerationStatus =
  | { kind: "idle" }
  | { kind: "instant"; source: "rules" | "cache" }
  | { kind: "writing" }
  | { kind: "done"; firstTokenMs: number | null; totalMs: number }
  | { kind: "error"; message: string; fixInSettings: boolean };

export interface SqlGenerationOptions {
  connectionId: string | null;
  schema: Schema | null;
  model: string;
  hasApiKey: boolean;
  /** Receives the SQL as it streams in, and the final SQL. */
  onSql: (sql: string) => void;
  /** Receives the finished SQL when a run was requested while it was still being written. */
  onRunRequested: (sql: string) => void;
}

/** Strips Markdown fences the model may add despite instructions. */
function cleanSql(s: string): string {
  return s
    .replace(/^\s*```(?:sql)?\s*\n?/i, "")
    .replace(/\n?```\s*$/, "")
    .trimStart();
}

const normalize = (q: string) => q.trim().toLowerCase().replace(/\s+/g, " ");

/**
 * Turns the question into SQL as the person types:
 * instant local answers for simple requests, a per-question cache, and otherwise a
 * debounced, streamed AI request. Each new request supersedes the previous one.
 */
export function useSqlGeneration(options: SqlGenerationOptions) {
  const [question, setQuestionState] = useState("");
  const [status, setStatus] = useState<GenerationStatus>({ kind: "idle" });

  const opts = useLatest(options);
  const statusRef = useLatest(status);
  const reqId = useRef(0);
  const buffer = useRef("");
  const firstTokenMs = useRef<number | null>(null);
  const inFlightQuestion = useRef("");
  const runWhenReady = useRef(false);
  const debounce = useRef<number | undefined>(undefined);
  const cache = useRef(new Map<string, string>());

  const cacheKey = (q: string) => `${opts.current.connectionId}|${opts.current.model}|${normalize(q)}`;

  const clearDebounce = () => {
    window.clearTimeout(debounce.current);
    debounce.current = undefined;
  };

  const fail = useCallback((message: string) => {
    runWhenReady.current = false;
    setStatus({ kind: "error", message, fixInSettings: message.includes("Settings") });
  }, []);

  /** Drops any request in flight; its events will be ignored. */
  const cancel = useCallback(() => {
    clearDebounce();
    reqId.current++;
    void commands.cancelGeneration();
  }, []);

  const start = useCallback(
    (q: string) => {
      const { connectionId, hasApiKey } = opts.current;
      clearDebounce();
      if (!connectionId) return;
      if (!hasApiKey) {
        fail("Add your Claude API key in Settings to turn English into SQL.");
        return;
      }
      const id = ++reqId.current;
      buffer.current = "";
      firstTokenMs.current = null;
      inFlightQuestion.current = q;
      setStatus({ kind: "writing" });
      commands.generateSql(connectionId, q, id).catch((e) => id === reqId.current && fail(errorText(e)));
    },
    [fail, opts],
  );

  useEffect(() => {
    const unsubscribe = subscribeToAi({
      delta: (e) => {
        if (e.reqId !== reqId.current) return;
        if (e.firstTokenMs != null) firstTokenMs.current = e.firstTokenMs;
        buffer.current += e.text;
        opts.current.onSql(cleanSql(buffer.current));
      },
      reset: (id) => {
        if (id === reqId.current) buffer.current = "";
      },
      done: (e) => {
        if (e.reqId !== reqId.current) return;
        if (e.error) return fail(e.error);
        const sql = cleanSql(buffer.current).trimEnd();
        opts.current.onSql(sql);
        cache.current.set(cacheKey(inFlightQuestion.current), sql);
        // Still typing: a newer request is about to start, so stay in "writing".
        if (debounce.current !== undefined) return;
        setStatus({ kind: "done", firstTokenMs: firstTokenMs.current, totalMs: e.totalMs });
        if (runWhenReady.current) {
          runWhenReady.current = false;
          opts.current.onRunRequested(sql);
        }
      },
    });
    return () => void unsubscribe.then((off) => off());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const setQuestion = useCallback(
    (text: string) => {
      setQuestionState(text);
      runWhenReady.current = false;
      clearDebounce();
      if (!text.trim()) {
        cancel();
        setStatus({ kind: "idle" });
        return;
      }
      const instant = fastPath(text, opts.current.schema);
      const cached = cache.current.get(cacheKey(text));
      if (instant ?? cached) {
        cancel();
        opts.current.onSql((instant ?? cached)!);
        setStatus({ kind: "instant", source: instant ? "rules" : "cache" });
        return;
      }
      setStatus({ kind: "writing" });
      debounce.current = window.setTimeout(() => start(text), DEBOUNCE_MS);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [cancel, start],
  );

  // A different database: start over.
  useEffect(() => {
    cancel();
    setQuestionState("");
    setStatus({ kind: "idle" });
  }, [options.connectionId, cancel]);

  /**
   * Call when the person asks to run. Returns true if the SQL is still being written —
   * it will then be passed to `onRunRequested` once finished.
   */
  const requestRun = useCallback((): boolean => {
    if (statusRef.current.kind !== "writing") return false;
    runWhenReady.current = true;
    if (debounce.current !== undefined) start(question); // skip the remaining debounce wait
    return true;
  }, [question, start, statusRef]);

  const retry = useCallback(() => question.trim() && start(question), [question, start]);

  // An API key was just added: retry the question that was waiting for it.
  useEffect(() => {
    if (options.hasApiKey && statusRef.current.kind === "error" && statusRef.current.fixInSettings) retry();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [options.hasApiKey]);

  return { question, setQuestion, status, requestRun, retry };
}
