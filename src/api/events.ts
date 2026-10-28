import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AgentEvent, AiDelta, AiDone, AuditEvent, VoiceProgress } from "./types";

/** Must match EVENT in src-tauri/src/commands/agent.rs. */
export const AGENT_EVENT = "agent://event";

/** Subscribes to agent mode's events; filter them by `runId`. */
export function subscribeToAgent(handler: (e: AgentEvent) => void): Promise<UnlistenFn> {
  return listen<AgentEvent>(AGENT_EVENT, (e) => handler(e.payload));
}

export interface AiEventHandlers {
  delta: (e: AiDelta) => void;
  done: (e: AiDone) => void;
  /** The first model declined and a fallback model restarts the answer. */
  reset: (reqId: number) => void;
}

/** Subscribes to the AI streaming events emitted by src-tauri/src/ai/mod.rs. */
export async function subscribeToAi(handlers: AiEventHandlers): Promise<UnlistenFn> {
  const unlisten = await Promise.all([
    listen<AiDelta>("ai://delta", (e) => handlers.delta(e.payload)),
    listen<AiDone>("ai://done", (e) => handlers.done(e.payload)),
    listen<{ reqId: number }>("ai://reset", (e) => handlers.reset(e.payload.reqId)),
  ]);
  return () => unlisten.forEach((u) => u());
}

/** Calls `onNew` when an AI app hands work to SayQL; fetch it with `commands.takeHandoffs`. */
export function subscribeToHandoffs(onNew: () => void): Promise<UnlistenFn> {
  return listen("handoff://new", onNew);
}

export interface VoiceEventHandlers {
  progress?: (e: VoiceProgress) => void;
  /** Microphone loudness, 0–1, about 20 times a second while recording. */
  level?: (level: number) => void;
  /** The system is withholding the microphone's audio (access denied). */
  blocked?: (message: string) => void;
}

/** Subscribes to the voice input events emitted by src-tauri/src/commands/voice.rs. */
export async function subscribeToVoice(handlers: VoiceEventHandlers): Promise<UnlistenFn> {
  const unlisten = await Promise.all([
    listen<VoiceProgress>("voice://progress", (e) => handlers.progress?.(e.payload)),
    listen<number>("voice://level", (e) => handlers.level?.(e.payload)),
    listen<string>("voice://blocked", (e) => handlers.blocked?.(e.payload)),
  ]);
  return () => unlisten.forEach((u) => u());
}

/** Must match EVENT in src-tauri/src/commands/audit.rs. */
export const AUDIT_EVENT = "audit://event";

/** Subscribes to "Audit with…" runs' progress; filter it by `runId`. */
export function subscribeToAudit(handler: (e: AuditEvent) => void): Promise<UnlistenFn> {
  return listen<AuditEvent>(AUDIT_EVENT, (e) => handler(e.payload));
}
