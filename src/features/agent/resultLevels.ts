import type { AgentResults, AppResults } from "../../api";

/**
 * One vocabulary for what the AI gets back from a query, shared by agent mode (a connection's
 * agent settings) and AI apps over MCP (Settings → AI apps → Access). The stored values stay as
 * they were (`AgentResults`, `AppResults`); only the words are shared.
 *
 * Every line here has to match the backend:
 * - agent mode: src-tauri/src/ai/agent/run.rs (`Results::Off | Counts | Rows`; Rows sends up to
 *   `ROWS_SENT` = 20 rows per query through the masker),
 * - AI apps: src-tauri/src/mcp.rs `shape` (`AppResults::Off | Counts | Masked | All`).
 * Defaults (src-tauri/src/store/agent.rs): agent mode `off`, AI apps `counts`.
 */
export type ResultLevel = "nothing" | "counts" | "masked" | "all";

export interface LevelText {
  /** The name, the same wherever the level is chosen or shown. */
  label: string;
  /** Lower-case, for a sentence or a status line: "Results to AI: counts and summaries". */
  short: string;
  /** One line on what is sent. */
  detail: string;
}

export const LEVELS: Record<ResultLevel, LevelText> = {
  nothing: {
    label: "Nothing (names and row counts only)",
    short: "nothing (names and row counts only)",
    detail: "Table and column names and how many rows a query returned. No values from your tables.",
  },
  counts: {
    label: "Counts and summaries",
    short: "counts and summaries",
    detail:
      "Row counts and, per column, its type, nulls, distinct values, and min and max for numbers and dates. No row values or group labels.",
  },
  masked: {
    label: "Rows, masked",
    short: "rows, masked",
    detail:
      "Rows, with likely personal data (emails, phones, names, addresses, IDs) replaced by tokens before they leave. Best effort: free text can hold anything.",
  },
  all: {
    label: "All rows",
    short: "all rows",
    detail: "Rows as they are, and CSV export. Never-send columns are still left out. For data you're happy to share with that app.",
  },
};

/** Agent mode's stored values, as levels. Its `rows` is masked rows, at most 20 per query. */
export const AGENT_LEVEL: Record<AgentResults, ResultLevel> = { off: "nothing", counts: "counts", rows: "masked" };
/** AI apps' stored values, as levels. */
export const APP_LEVEL: Record<AppResults, ResultLevel> = { off: "nothing", counts: "counts", masked: "masked", all: "all" };

/** What agent mode starts with on a connection (`Results::default()`). */
export const AGENT_DEFAULT: AgentResults = "off";
/** What AI apps start with on a connection (`AppResults::default()`). */
export const APP_DEFAULT: AppResults = "counts";

export interface LevelOption<T extends string> {
  value: T;
  level: ResultLevel;
  label: string;
  detail: string;
  /** The level a connection has until the person changes it. */
  isDefault: boolean;
}

/** Agent mode's choices, in order. */
export const AGENT_OPTIONS: LevelOption<AgentResults>[] = (["off", "counts", "rows"] as const).map((value) => {
  const level = AGENT_LEVEL[value];
  return {
    value,
    level,
    label: value === "rows" ? `${LEVELS.masked.label} (up to 20 per query)` : LEVELS[level].label,
    detail:
      value === "rows"
        ? `${LEVELS.masked.detail} SayQL fills the real values back in on your screen.`
        : value === "off"
          ? "The AI sees your question, the schema and how many rows each query returned. No values from your tables."
          : LEVELS[level].detail,
    isDefault: value === AGENT_DEFAULT,
  };
});

/** AI apps' choices, in order. */
export const APP_OPTIONS: LevelOption<AppResults>[] = (["off", "counts", "masked", "all"] as const).map((value) => {
  const level = APP_LEVEL[value];
  return { value, level, label: LEVELS[level].label, detail: LEVELS[level].detail, isDefault: value === APP_DEFAULT };
});

/** A choice's label with "(default)" after the default one, for pickers. */
export const withDefault = (o: LevelOption<string>) => (o.isDefault ? `${o.label} · default` : o.label);

/** "Results to AI: counts and summaries", for agent mode's status line. */
export function agentLevelShort(results: AgentResults): string {
  return results === "rows" ? `${LEVELS.masked.short} (up to 20 per query)` : LEVELS[AGENT_LEVEL[results]].short;
}

/** For AI apps' status or activity lines. */
export const appLevelShort = (apps: AppResults) => LEVELS[APP_LEVEL[apps]].short;

/**
 * Where never-send columns apply, in one sentence. They're enforced for agent mode
 * (src-tauri/src/ai/agent/mask.rs) and for AI apps over MCP (src-tauri/src/mcp.rs). The Ask box
 * sends no values from your tables (src-tauri/src/commands/generation.rs: the question, the
 * schema, notes and the editor's SQL), so there is nothing for them to hold back there.
 */
export const NEVER_SEND_SCOPE =
  "Never-send columns apply to Investigate and to AI apps over MCP (Claude Code, Cursor, …): queries that use them are refused. The Ask box sends no values from your tables, so they don't change it.";
