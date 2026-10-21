import { invoke } from "@tauri-apps/api/core";
import type { Bootstrap, ConnectionConfig, DatabaseList, QueryResult, Schema, Settings, Target } from "./types";

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

  generateSql: (t: Target, question: string, reqId: number) =>
    invoke<void>("generate_sql", { id: t.connectionId, database: t.database, question, reqId }),
  cancelGeneration: () => invoke<void>("cancel_generation"),

  saveSettings: (model: string, apiKey: string | null, workspaceId: string) =>
    invoke<Settings>("save_settings", { model, apiKey, workspaceId }),
};
