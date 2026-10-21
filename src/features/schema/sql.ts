// SQL text helpers that depend on the schema/dialect. Shared by the fast path and table previews.
import type { Dialect, Table } from "../../api";

const RESERVED = new Set([
  "user", "order", "group", "table", "select", "where", "from", "limit", "offset", "desc", "asc",
  "key", "index", "check", "default", "column", "values", "references", "primary", "case", "end",
]);

/** The name shown in the UI and used in prompts: `schema.table`, except for Postgres `public`. */
export function qualifiedName(t: Table): string {
  return t.schema && t.schema !== "public" ? `${t.schema}.${t.name}` : t.name;
}

/** Quotes an identifier only when it needs it (mixed case, symbols, reserved words). */
export function quoteIdent(ident: string, dialect: Dialect): string {
  if (/^[a-z_][a-z0-9_]*$/.test(ident) && !RESERVED.has(ident)) return ident;
  return dialect === "MySQL" ? `\`${ident.replace(/`/g, "``")}\`` : `"${ident.replace(/"/g, '""')}"`;
}

/** A table reference usable in FROM. */
export function tableRef(t: Table, dialect: Dialect): string {
  const name = quoteIdent(t.name, dialect);
  return t.schema && t.schema !== "public" && dialect === "PostgreSQL" ? `${quoteIdent(t.schema, dialect)}.${name}` : name;
}

export function selectAllSql(t: Table, dialect: Dialect, limit = 100): string {
  return `SELECT *\nFROM ${tableRef(t, dialect)}\nLIMIT ${limit};`;
}
