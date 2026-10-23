import { useCallback, useEffect, useReducer } from "react";
import { commands, subscribeToAgent, type AgentReply, type Target } from "../../api";
import { useLatest } from "../../hooks/useLatest";
import { errorText } from "../../lib/errors";
import { IDLE, agentReducer, isActive, type AgentRunState } from "./agentRun";

export interface AgentRunOptions {
  target: Target | null;
  /** The tab id: one run per tab, and a new run stops the one before. */
  slot: string;
  /** A finished run the tab kept (queries and answer, no rows). */
  initial?: AgentRunState | null;
}

export interface StartOptions {
  focusTables: string[];
  skill: string | null;
  /** Continue the tab's last run instead of starting fresh. */
  followUp: boolean;
}

/** Starts, watches, answers and stops agent runs in one tab. */
export function useAgentRun({ target, slot, initial }: AgentRunOptions) {
  const [state, dispatch] = useReducer(agentReducer, initial ?? IDLE);
  const latest = useLatest(state);

  useEffect(() => {
    const off = subscribeToAgent((event) => dispatch({ type: "event", event }));
    return () => void off.then((f) => f());
  }, []);

  const start = useCallback(
    (question: string, o: StartOptions) => {
      if (!target || !question.trim()) return;
      const runId = crypto.randomUUID();
      dispatch({ type: "start", runId, question, keepSteps: o.followUp });
      commands
        .startAgent(target, question, runId, slot, o.focusTables, o.skill, o.followUp)
        .catch((e) => dispatch({ type: "failed", runId, error: errorText(e) }));
    },
    [target, slot],
  );

  const stop = useCallback(() => {
    const id = latest.current.runId;
    if (id && isActive(latest.current)) void commands.stopAgent(id);
  }, [latest]);

  const reply = useCallback(
    (r: AgentReply) => {
      const id = latest.current.runId;
      if (!id) return;
      dispatch({ type: "replied" });
      commands.replyAgent(id, r).catch((e) => dispatch({ type: "failed", runId: id, error: errorText(e) }));
    },
    [latest],
  );

  // A tab closed mid-run stops its run.
  useEffect(() => () => {
    const s = latest.current;
    if (s.runId && isActive(s)) void commands.stopAgent(s.runId);
  }, [latest]);

  return {
    state,
    start,
    stop,
    go: (plan: string[] | null) => reply({ type: "go", plan }),
    send: () => reply({ type: "send" }),
    skip: () => reply({ type: "skip" }),
    answer: (text: string) => reply({ type: "answer", text }),
    clear: () => dispatch({ type: "clear" }),
    active: isActive(state),
  };
}

export type AgentRun = ReturnType<typeof useAgentRun>;
