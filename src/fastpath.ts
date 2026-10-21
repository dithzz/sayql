// Instant SQL for simple requests ("show customers", "how many orders", "latest 20 reviews"),
// built from the cached schema with no network call. Anything less certain goes to the AI.
import { qualifiedName, type Schema, type Table } from "./api";

const RESERVED = new Set([
  "user", "order", "group", "table", "select", "where", "from", "limit", "offset", "desc", "asc",
  "key", "index", "check", "default", "column", "values", "references", "primary", "case", "end",
]);

function quote(ident: string, dialect: Schema["dialect"]): string {
  if (/^[a-z_][a-z0-9_]*$/.test(ident) && !RESERVED.has(ident)) return ident;
  return dialect === "MySQL" ? `\`${ident.replace(/`/g, "``")}\`` : `"${ident.replace(/"/g, '""')}"`;
}

function tableRef(t: Table, dialect: Schema["dialect"]): string {
  const name = quote(t.name, dialect);
  return t.schema && t.schema !== "public" && dialect === "PostgreSQL" ? `${quote(t.schema, dialect)}.${name}` : name;
}

function norm(s: string): string {
  return s.toLowerCase().replace(/[_\s]+/g, " ").trim();
}

function variants(name: string): string[] {
  const n = norm(name);
  const out = new Set([n]);
  if (n.endsWith("ies")) out.add(n.slice(0, -3) + "y");
  else if (n.endsWith("s")) out.add(n.slice(0, -1));
  else out.add(n.endsWith("y") ? n.slice(0, -1) + "ies" : n + "s");
  return [...out];
}

/** The single table whose name matches `phrase`, or null when none or several match. */
function findTable(schema: Schema, phrase: string): Table | null {
  const p = norm(phrase);
  const hits = schema.tables.filter(
    (t) => variants(t.name).includes(p) || variants(qualifiedName(t).replace(".", " ")).includes(p),
  );
  return hits.length === 1 ? hits[0] : null;
}

function orderColumn(t: Table): string | null {
  const byName = t.columns.find((c) => /^(created|inserted|ordered|signed_up|updated)(_at|_on|_date)?$/i.test(c.name));
  return (byName ?? t.columns.find((c) => c.primary_key))?.name ?? null;
}

export function previewSql(t: Table, dialect: Schema["dialect"]): string {
  return `SELECT *\nFROM ${tableRef(t, dialect)}\nLIMIT 100;`;
}

export function fastPath(question: string, schema: Schema | null): string | null {
  if (!schema) return null;
  const q = question
    .trim()
    .toLowerCase()
    .replace(/[?.!]+$/, "")
    .replace(/\s+/g, " ");
  if (!q) return null;
  const d = schema.dialect;
  let m: RegExpMatchArray | null;

  // "count orders", "how many customers are there", "number of reviews"
  if ((m = q.match(/^(?:count(?: of)?(?: all)?|how many|number of|total number of) (?:the )?(.+?)(?: are there| do we have| exist| in total| total)?$/))) {
    const t = findTable(schema, m[1]);
    if (t) return `SELECT COUNT(*) AS total_${t.name.toLowerCase().replace(/[^a-z0-9_]/g, "_")}\nFROM ${tableRef(t, d)};`;
  }

  // "latest 20 orders", "first 5 products". ("top 10 customers" implies a ranking, so it goes to the AI.)
  const which = "first|last|latest|newest|recent|most recent";
  if ((m = q.match(new RegExp(`^(?:show |list |get |display )?(?:me )?(?:the )?(${which})? ?(\\d{1,5}) (?:(${which}) )?(.+)$`)))) {
    const t = findTable(schema, m[4]);
    const n = Math.min(parseInt(m[2], 10), 10000);
    const qualifier = m[1] ?? m[3];
    if (t && n > 0) {
      const newest = qualifier && /last|latest|newest|recent/.test(qualifier);
      const col = newest ? orderColumn(t) : null;
      if (newest && !col) return null;
      return `SELECT *\nFROM ${tableRef(t, d)}${col ? `\nORDER BY ${quote(col, d)} DESC` : ""}\nLIMIT ${n};`;
    }
  }

  // "customers", "show all products", "list the order items"
  if ((m = q.match(/^(?:show|list|get|display|see|view|open|select)? ?(?:me )?(?:all |every |the )*(?:rows (?:in|from) )?(.+?)(?: table)?$/))) {
    const t = findTable(schema, m[1]);
    if (t) return `SELECT *\nFROM ${tableRef(t, d)}\nLIMIT 100;`;
  }
  return null;
}
