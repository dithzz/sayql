import { useSyncExternalStore } from "react";
import { storage } from "../../lib/storage";

const KEY = "sayql.agentAuto";
let auto = storage.get(KEY, false);
const listeners = new Set<() => void>();

/** Sets Auto mode for every tab, and remembers it. */
export function setAgentAuto(on: boolean) {
  auto = on;
  storage.set(KEY, on);
  listeners.forEach((l) => l());
}

/**
 * Auto mode: agent runs don't stop for the plan or before each send, like a coding agent's
 * auto-accept. What the AI may see stays as the connection's setting; questions still ask.
 */
export function useAgentAuto(): [boolean, (on: boolean) => void] {
  const on = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => auto,
  );
  return [on, setAgentAuto];
}
