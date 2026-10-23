import { invoke } from "@tauri-apps/api/core";
import type {
  AgentAccess,
  AgentReply,
  AgentSettings,
  Bootstrap,
  DbNote,
  ConnectionConfig,
  DatabaseList,
  GenerationTask,
  Handoff,
  McpSetup,
  QueryResult,
  Script,
  Skill,
  Schema,
  ProviderChoice,
  Settings,
  UpdateInfo,
  AppInfo,
  Target,
  VoiceStatus,
} from "./types";

/** Typed wrappers for the Tauri commands registered in src-tauri/src/lib.rs. */
export const commands = {
  init: () => invoke<Bootstrap>("init"),

  saveConnection: (config: ConnectionConfig, password: string | null) =>
    invoke<ConnectionConfig>("save_connection", { config, password }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),
  testConnection: (config: ConnectionConfig, password: string | null) =>
    invoke<string>("test_connection", { config, password }),
  openDatabase: (t: Target) => invoke<Schema>("open_database", { id: t.connectionId, database: t.database }),
  refreshSchema: (t: Target) => invoke<Schema>("refresh_schema", { id: t.connectionId, database: t.database }),
  /** `null` for SQLite, where a file is a single database. */
  listDatabases: (connectionId: string) => invoke<DatabaseList | null>("list_databases", { id: connectionId }),

  /** `runId` lets `cancelQuery` stop the query while it runs. */
  runQuery: (t: Target, sql: string, allowWrite: boolean, runId?: string) =>
    invoke<QueryResult>("run_query", { id: t.connectionId, database: t.database, sql, allowWrite, runId }),
  /** Stops the query started with `runId` on the server; it then fails with QUERY_CANCELLED. */
  cancelQuery: (runId: string) => invoke<void>("cancel_query", { runId }),

  /**
   * Asks the AI for SQL; the answer streams back as `ai://` events (see `subscribeToAi`).
   * - `slot` scopes cancellation: a new request replaces the previous one in the same slot (editor tab).
   * - `focusTables`: qualified table names the person picked; drafts see only those and their neighbours.
   * - `skill`: the slug of a saved skill whose instruction goes with the request.
   */
  generateSql: (t: Target, question: string, reqId: number, slot: string, task: GenerationTask, focusTables: string[], skill: string | null) =>
    invoke<void>("generate_sql", { id: t.connectionId, database: t.database, question, reqId, slot, task, focusTables, skill }),
  cancelGeneration: (slot: string) => invoke<void>("cancel_generation", { slot }),

  /**
   * Starts an agent run in `slot` (the tab), stopping the one there; progress arrives as
   * `agent://event`s (see `subscribeToAgent`). `followUp` continues the tab's last run.
   */
  startAgent: (t: Target, question: string, runId: string, slot: string, focusTables: string[], skill: string | null, followUp: boolean) =>
    invoke<void>("start_agent", { id: t.connectionId, database: t.database, question, runId, slot, focusTables, skill, followUp }),
  /** Stops the model and the running query, on the server too. */
  stopAgent: (runId: string) => invoke<void>("stop_agent", { runId }),
  replyAgent: (runId: string, reply: AgentReply) => invoke<void>("reply_agent", { runId, reply }),
  agentAccess: (connectionId: string) => invoke<AgentAccess>("agent_access", { id: connectionId }),
  saveAgentAccess: (connectionId: string, access: AgentAccess) => invoke<AgentAccess>("save_agent_access", { id: connectionId, access }),
  listNotes: (t: Target) => invoke<DbNote[]>("list_notes", { id: t.connectionId, database: t.database }),
  /** Creates the note when `id` is empty; refuses notes that hold values from the data. */
  saveNote: (note: DbNote) => invoke<DbNote>("save_note", { note }),
  deleteNote: (id: string) => invoke<void>("delete_note", { id }),
  /** The agent model (empty: the expert model) and the run limits. */
  saveAgentSettings: (agentModel: string, agent: AgentSettings) => invoke<Settings>("save_agent_settings", { agentModel, agent }),

  listScripts: () => invoke<Script[]>("list_scripts"),
  /** Creates the script when `id` is empty; returns it as stored. */
  saveScript: (script: Script) => invoke<Script>("save_script", { script }),
  deleteScript: (id: string) => invoke<void>("delete_script", { id }),

  listSkills: () => invoke<Skill[]>("list_skills"),
  /** Creates a skill, or replaces the one whose slug was `original`; returns it as stored. */
  saveSkill: (skill: Skill, original: string | null) => invoke<Skill>("save_skill", { skill, original }),
  deleteSkill: (slug: string) => invoke<void>("delete_skill", { slug }),

  mcpSetup: () => invoke<McpSetup>("mcp_setup"),
  /** Handoffs from AI apps waiting to be shown (see the `handoff://new` event). */
  takeHandoffs: () => invoke<Handoff[]>("take_handoffs"),

  /** Version, OS and chip, for pre-filling feedback. */
  appInfo: () => invoke<AppInfo>("app_info"),

  /** A newer release, or null (also when offline, and always in development builds). */
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  /** Downloads and installs the update, then restarts the app. */
  installUpdate: () => invoke<void>("install_update"),

  /** Saves the choices for one provider and makes it the one that writes SQL. */
  saveSettings: (choice: ProviderChoice) => invoke<Settings>("save_settings", { ...choice }),
  /** Marks the first-run setup as done so it isn't shown again. */
  finishOnboarding: () => invoke<Settings>("finish_onboarding"),
  /** Model ids an AI provider offers; also checks the key and base URL. */
  listModels: (provider: string, baseUrl: string, apiKey: string | null) =>
    invoke<string[]>("list_models", { provider, baseUrl, apiKey }),

  /** Voice input: whether the on-device speech model is downloaded. */
  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  /** Downloads the speech model once; progress arrives as `voice://progress` events. */
  voiceDownload: () => invoke<void>("voice_download"),
  voiceCancelDownload: () => invoke<void>("voice_cancel_download"),
  voiceDeleteModel: () => invoke<void>("voice_delete_model"),
  /** Starts recording from the default microphone. */
  voiceStart: () => invoke<void>("voice_start"),
  /** Stops recording and returns the transcript. `prompt`: table and column names to help spelling. */
  voiceStop: (prompt: string) => invoke<string>("voice_stop", { prompt }),
  /** Stops recording and discards the audio. */
  voiceCancel: () => invoke<void>("voice_cancel"),
};
