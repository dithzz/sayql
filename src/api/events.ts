import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AiDelta, AiDone } from "./types";

export interface AiEventHandlers {
  delta: (e: AiDelta) => void;
  done: (e: AiDone) => void;
  /** The first model declined and a fallback model restarts the answer. */
  reset: (reqId: number) => void;
}

/** Subscribes to the AI streaming events emitted by src-tauri/src/ai.rs. */
export async function subscribeToAi(handlers: AiEventHandlers): Promise<UnlistenFn> {
  const unlisten = await Promise.all([
    listen<AiDelta>("ai://delta", (e) => handlers.delta(e.payload)),
    listen<AiDone>("ai://done", (e) => handlers.done(e.payload)),
    listen<{ reqId: number }>("ai://reset", (e) => handlers.reset(e.payload.reqId)),
  ]);
  return () => unlisten.forEach((u) => u());
}
