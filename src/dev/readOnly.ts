// Development only: a copy of the backend's read-only check, so the browser preview asks
// for write confirmation on the same statements the app does. The source of truth is
// `is_read_only` in src-tauri/src/db/classify.rs; keep this in step with it.

/** Same list as `WRITE_KEYWORDS` in classify.rs. */
const WRITE_KEYWORDS = new Set([
  "INSERT", "UPDATE", "DELETE", "MERGE", "UPSERT", "INTO", "DROP", "ALTER", "CREATE", "TRUNCATE", "RENAME",
  "GRANT", "REVOKE", "DENY", "EXEC", "EXECUTE", "CALL", "COMMIT", "ROLLBACK", "SAVEPOINT", "BEGIN", "BACKUP",
  "RESTORE", "DBCC", "KILL", "SHUTDOWN", "RECONFIGURE", "BULK", "OPENROWSET", "OPENQUERY", "ATTACH", "DETACH",
  "VACUUM", "REINDEX", "COPY", "LOCK", "USE", "SET", "CHECKPOINT", "RECEIVE",
]);

const READ_STARTS = new Set(["SELECT", "WITH", "EXPLAIN", "SHOW", "DESCRIBE", "DESC", "VALUES", "TABLE"]);

/** True when the statement only reads data (see `is_read_only` in classify.rs). */
export function isReadOnly(sql: string): boolean {
  const s = stripLeadingComments(sql);
  const first = (s.split(/[\s(;]/).find((w) => w !== "") ?? "").toUpperCase();
  // `PRAGMA x = y` changes settings; `PRAGMA x` / `PRAGMA x(y)` reads them.
  const reads = READ_STARTS.has(first) || (first === "PRAGMA" && !s.includes("="));
  return reads && !keywords(sql).some((w) => WRITE_KEYWORDS.has(w.toUpperCase()));
}

function stripLeadingComments(sql: string): string {
  let s = sql.trimStart();
  for (;;) {
    if (s.startsWith("--")) {
      const nl = s.indexOf("\n");
      s = nl < 0 ? "" : s.slice(nl + 1).trimStart();
    } else if (s.startsWith("/*")) {
      const end = s.indexOf("*/", 2);
      s = end < 0 ? "" : s.slice(end + 2).trimStart();
    } else {
      return s;
    }
  }
}

/** Bare words, skipping string literals, quoted identifiers (`"x"`, `[x]`, `` `x` ``) and comments. */
function keywords(sql: string): string[] {
  const words: string[] = [];
  // Index just past the next `end` at or after `from`, or the end of the text.
  const skipTo = (from: number, end: string) => {
    const p = sql.indexOf(end, from);
    return p < 0 ? sql.length : p + end.length;
  };
  let i = 0;
  while (i < sql.length) {
    const c = sql[i];
    if (c === "'") i = skipTo(i + 1, "'"); // '' escapes re-enter as an empty string
    else if (c === '"') i = skipTo(i + 1, '"');
    else if (c === "`") i = skipTo(i + 1, "`");
    else if (c === "[") i = skipTo(i + 1, "]");
    else if (c === "-" && sql[i + 1] === "-") i = skipTo(i, "\n");
    else if (c === "/" && sql[i + 1] === "*") i = skipTo(i + 2, "*/");
    else if (/[A-Za-z_]/.test(c)) {
      const start = i;
      while (i < sql.length && /[A-Za-z0-9_]/.test(sql[i])) i++;
      words.push(sql.slice(start, i));
    } else i++;
  }
  return words;
}
