import { invoke } from "@tauri-apps/api/core";
import type {
  Bootstrap,
  ConnectionConfig,
  DatabaseList,
  GenerationTask,
  McpSetup,
  QueryResult,
  Script,
  Schema,
  ProviderChoice,
  Settings,
  Target,
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

  runQuery: (t: Target, sql: string, allowWrite: boolean) =>
    invoke<QueryResult>("run_query", { id: t.connectionId, database: t.database, sql, allowWrite }),

  /** `slot` scopes cancellation: a new request replaces the previous one in the same slot (editor tab). */
  /** `focusTables`: qualified table names the person picked; drafts see only those and their neighbours. */
  generateSql: (t: Target, question: string, reqId: number, slot: string, task: GenerationTask, focusTables: string[]) =>
    invoke<void>("generate_sql", { id: t.connectionId, database: t.database, question, reqId, slot, task, focusTables }),
  cancelGeneration: (slot: string) => invoke<void>("cancel_generation", { slot }),

  listScripts: () => invoke<Script[]>("list_scripts"),
  /** Creates the script when `id` is empty; returns it as stored. */
  saveScript: (script: Script) => invoke<Script>("save_script", { script }),
  deleteScript: (id: string) => invoke<void>("delete_script", { id }),

  mcpSetup: () => invoke<McpSetup>("mcp_setup"),

  /** Saves the choices for one provider and makes it the one that writes SQL. */
  saveSettings: (choice: ProviderChoice) => invoke<Settings>("save_settings", { ...choice }),
  /** Model ids an AI provider offers; also checks the key and base URL. */
  listModels: (provider: string, baseUrl: string, apiKey: string | null) =>
    invoke<string[]>("list_models", { provider, baseUrl, apiKey }),
};
