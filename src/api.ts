import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Kind = "postgres" | "mysql" | "sqlite";

export interface ConnConfig {
  id: string;
  name: string;
  kind: Kind;
  host: string;
  port: number | null;
  database: string;
  user: string;
  file: string;
  ssl: boolean;
}

export interface Column {
  name: string;
  data_type: string;
  nullable: boolean;
  primary_key: boolean;
  enum_values: string[] | null;
}

export interface ForeignKey {
  column: string;
  ref_table: string;
  ref_column: string;
}

export interface Table {
  schema: string | null;
  name: string;
  kind: "table" | "view";
  columns: Column[];
  foreign_keys: ForeignKey[];
  row_estimate: number | null;
}

export interface Schema {
  dialect: "PostgreSQL" | "MySQL" | "SQLite";
  tables: Table[];
}

export type Cell = string | number | boolean | null | object;

export interface QueryResult {
  columns: string[];
  rows: Cell[][];
  rowsAffected: number | null;
  truncated: boolean;
  elapsedMs: number;
}

export interface ModelOption {
  id: string;
  label: string;
}

export interface Settings {
  model: string;
  hasApiKey: boolean;
  workspaceId: string;
  models: ModelOption[];
}

export const api = {
  init: () => invoke<{ connections: ConnConfig[]; settings: Settings }>("init"),
  saveConnection: (config: ConnConfig, password: string | null) =>
    invoke<ConnConfig>("save_connection", { config, password }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),
  testConnection: (config: ConnConfig, password: string | null) =>
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

export interface AiDelta {
  reqId: number;
  text: string;
  firstTokenMs: number | null;
}

export interface AiDone {
  reqId: number;
  error: string | null;
  totalMs: number;
  cacheReadTokens: number | null;
}

export async function onAiEvents(handlers: {
  delta: (e: AiDelta) => void;
  done: (e: AiDone) => void;
  reset: (reqId: number) => void;
}): Promise<UnlistenFn> {
  const un = await Promise.all([
    listen<AiDelta>("ai://delta", (e) => handlers.delta(e.payload)),
    listen<AiDone>("ai://done", (e) => handlers.done(e.payload)),
    listen<{ reqId: number }>("ai://reset", (e) => handlers.reset(e.payload.reqId)),
  ]);
  return () => un.forEach((u) => u());
}

export function qualifiedName(t: Table): string {
  return t.schema && t.schema !== "public" ? `${t.schema}.${t.name}` : t.name;
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
