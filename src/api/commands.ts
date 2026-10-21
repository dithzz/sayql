import { invoke } from "@tauri-apps/api/core";
import type { Bootstrap, ConnectionConfig, QueryResult, Schema, Settings } from "./types";

/** Typed wrappers for the Tauri commands registered in src-tauri/src/lib.rs. */
export const commands = {
  init: () => invoke<Bootstrap>("init"),

  saveConnection: (config: ConnectionConfig, password: string | null) =>
    invoke<ConnectionConfig>("save_connection", { config, password }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),
  testConnection: (config: ConnectionConfig, password: string | null) =>
    invoke<string>("test_connection", { config, password }),
  openConnection: (id: string) => invoke<Schema>("open_connection", { id }),
  refreshSchema: (id: string) => invoke<Schema>("refresh_schema", { id }),

  runQuery: (id: string, sql: string, allowWrite: boolean) =>
    invoke<QueryResult>("run_query", { id, sql, allowWrite }),

  generateSql: (id: string, question: string, reqId: number) =>
    invoke<void>("generate_sql", { id, question, reqId }),
  cancelGeneration: () => invoke<void>("cancel_generation"),

  saveSettings: (model: string, apiKey: string | null, workspaceId: string) =>
    invoke<Settings>("save_settings", { model, apiKey, workspaceId }),
};
