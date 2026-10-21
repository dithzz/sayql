// Shapes returned by the Rust backend (src-tauri/src). Keep in sync with the serde structs there.

export type DatabaseKind = "postgres" | "mysql" | "sqlite" | "mssql";

export interface ConnectionConfig {
  id: string;
  name: string;
  kind: DatabaseKind;
  host: string;
  port: number | null;
  database: string;
  user: string;
  /** SQLite database file path. */
  file: string;
  /** Require an encrypted connection. */
  ssl: boolean;
  /** SQL Server: accept a self-signed server certificate. */
  trustCert: boolean;
}

export type Dialect = "PostgreSQL" | "MySQL" | "SQLite" | "SQL Server";

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
  dialect: Dialect;
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

/** An AI service that can write SQL; mirrors `ProviderView` in src-tauri/src/commands/settings.rs. */
export interface AiProvider {
  id: string;
  label: string;
  note: string;
  /** Claude's own API, or the OpenAI-compatible API every other provider speaks. */
  api: "anthropic" | "openai";
  key: "required" | "optional" | "none";
  keyPlaceholder: string;
  /** Runs on this computer, so nothing is sent over the internet. */
  local: boolean;
  /** Empty when the person must enter one. */
  defaultBaseUrl: string;
  /** Empty means the default. */
  baseUrl: string;
  typingModel: string;
  /** Empty means the typing model does fixes too. */
  expertModel: string;
  hasKey: boolean;
}

export interface Settings {
  /** The provider that writes SQL. */
  provider: string;
  /** Writes SQL while the person types. */
  typingModel: string;
  /** Fixes rejected queries and improves queries on request. */
  expertModel: string;
  /** The provider has what it needs (key, model) to write SQL. */
  aiReady: boolean;
  /** The first-run setup was finished or dismissed. */
  onboarded: boolean;
  workspaceId: string;
  /** Claude's models. */
  models: ModelOption[];
  providers: AiProvider[];
}

/** What Settings saves for one provider. `apiKey: null` keeps the saved key. */
export interface ProviderChoice {
  provider: string;
  typingModel: string;
  expertModel: string;
  baseUrl: string;
  apiKey: string | null;
  workspaceId: string;
}

export interface DatabaseList {
  /** Databases this login can open, sorted by name. */
  databases: string[];
  /** The database the connection opens by default. */
  current: string | null;
}

/** One database on a saved connection. `database: null` is the connection's default database. */
export interface Target {
  connectionId: string;
  database: string | null;
}

/** The query last run in a tab; a follow-up request ("remove the middle name") edits it. */
export interface PreviousQuery {
  question: string;
  sql: string;
}

/** What the AI is asked to do; mirrors `ai::Task` in src-tauri/src/ai/prompt.rs. */
export type GenerationTask =
  | { kind: "draft"; previous: PreviousQuery | null }
  | { kind: "fix"; sql: string; error: string }
  | { kind: "improve"; sql: string };

/** A newer SayQL release on the update feed. */
export interface UpdateInfo {
  version: string;
  notes: string | null;
}

/** Where SayQL's MCP server binary is, for connecting Claude. */
export interface McpSetup {
  path: string;
  exists: boolean;
}

/** A query saved from a tab; mirrors `store::Script` in src-tauri/src/store/mod.rs. */
export interface Script {
  id: string;
  name: string;
  connectionId: string;
  database: string | null;
  question: string;
  sql: string;
  /** Milliseconds since the Unix epoch. */
  updatedAt: number;
}

export interface Bootstrap {
  connections: ConnectionConfig[];
  settings: Settings;
}

/** Sentinel error from `run_query`: the statement writes data and needs confirmation. */
export const WRITE_CONFIRM = "WRITE_CONFIRM";

export interface AiDelta {
  reqId: number;
  text: string;
  /** Present on the first chunk only. */
  firstTokenMs: number | null;
}

export interface AiDone {
  reqId: number;
  error: string | null;
  totalMs: number;
  cacheReadTokens: number | null;
}
