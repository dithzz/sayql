// SQL text helpers that depend on the schema/dialect. Shared by the fast path and table previews.
import type { Dialect, Table } from "../../api";

const RESERVED = new Set([
  "user", "order", "group", "table", "select", "where", "from", "limit", "offset", "desc", "asc",
  "key", "index", "check", "default", "column", "values", "references", "primary", "case", "end",
]);

/** The schema that needs no qualifier in each dialect. */
const DEFAULT_SCHEMA: Partial<Record<Dialect, string>> = { PostgreSQL: "public" };

/** The name shown in the UI and used in prompts: `schema.table`, except for Postgres `public`. */
export function qualifiedName(t: Table): string {
  return t.schema && t.schema !== "public" ? `${t.schema}.${t.name}` : t.name;
}

/**
 * Quotes an identifier only when it needs it: symbols, reserved words, and — in Postgres,
 * which folds unquoted names to lowercase — capital letters.
 */
export function quoteIdent(ident: string, dialect: Dialect): string {
  const plain = dialect === "PostgreSQL" ? /^[a-z_][a-z0-9_]*$/ : /^[a-z_][a-z0-9_]*$/i;
  if (plain.test(ident) && !RESERVED.has(ident.toLowerCase())) return ident;
  switch (dialect) {
    case "MySQL":
      return `\`${ident.replace(/`/g, "``")}\``;
    case "SQL Server":
      return `[${ident.replace(/]/g, "]]")}]`;
    default:
      return `"${ident.replace(/"/g, '""')}"`;
  }
}

/** A table reference usable in FROM. */
export function tableRef(t: Table, dialect: Dialect): string {
  const name = quoteIdent(t.name, dialect);
  const qualify = t.schema && t.schema !== DEFAULT_SCHEMA[dialect] && dialect !== "MySQL";
  return qualify ? `${quoteIdent(t.schema!, dialect)}.${name}` : name;
}

/** `SELECT * FROM t [ORDER BY ...]` limited to `n` rows, in the dialect's syntax. */
export function selectRowsSql(t: Table, dialect: Dialect, n: number, orderBy?: string): string {
  const order = orderBy ? `\nORDER BY ${quoteIdent(orderBy, dialect)} DESC` : "";
  return dialect === "SQL Server"
    ? `SELECT TOP ${n} *\nFROM ${tableRef(t, dialect)}${order};`
    : `SELECT *\nFROM ${tableRef(t, dialect)}${order}\nLIMIT ${n};`;
}

export function selectAllSql(t: Table, dialect: Dialect): string {
  return selectRowsSql(t, dialect, 100);
}
