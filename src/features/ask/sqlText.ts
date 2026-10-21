// Turns the model's raw reply into runnable SQL, even while it is still streaming.

/** Words a SQL statement can start with. A line starting otherwise, before any SQL, is prose. */
const STATEMENT_START =
  /^\s*(\(|select|with|insert|update|delete|merge|create|alter|drop|truncate|exec|execute|declare|set|use|explain|show|describe|desc|pragma|values|table|begin|call|grant|revoke)\b/i;

/**
 * Strips Markdown fences, and turns an explanation the model put before the SQL into
 * `--` comments, so the database (and Sayql's read-only check) see SQL first.
 */
export function cleanSql(raw: string): string {
  const text = raw
    .replace(/^\s*```(?:sql)?\s*\n?/i, "")
    .replace(/\n?```\s*$/, "")
    .trimStart();
  const lines = text.split("\n");
  let inSql = false;
  return lines
    .map((line) => {
      if (inSql) return line;
      const t = line.trim();
      if (STATEMENT_START.test(line)) {
        inSql = true;
        return line;
      }
      if (t === "" || t.startsWith("--") || t.startsWith("/*") || t.startsWith("*")) return line;
      return `-- ${t}`;
    })
    .join("\n");
}
