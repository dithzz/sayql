// What "Run selection" runs. A selection that cuts a statement in half would fail or, worse,
// run something else than meant (a DELETE without its WHERE), so it snaps to whole statements.

export interface Range {
  start: number;
  end: number;
}

/**
 * The statements in `sql`, as ranges split at `;` outside strings, quoted names and
 * comments. Each range ends after its `;` (or at the end of the text) and starts at its first
 * SQL (after blank space and comments). A part that is only comments isn't a statement.
 */
export function statementRanges(sql: string): Range[] {
  const out: Range[] = [];
  let start = 0;
  let i = 0;
  const push = (end: number) => {
    const text = sql.slice(start, end);
    const lead = text.length - stripLeading(text).length;
    if (stripLeading(text).replace(/^;$/, "")) out.push({ start: start + lead, end });
    start = end;
  };
  while (i < sql.length) {
    const c = sql[i];
    const next = sql[i + 1];
    if (c === "-" && next === "-") {
      const nl = sql.indexOf("\n", i);
      i = nl < 0 ? sql.length : nl + 1;
    } else if (c === "/" && next === "*") {
      const close = sql.indexOf("*/", i + 2);
      i = close < 0 ? sql.length : close + 2;
    } else if (c === "'" || c === '"' || c === "`") {
      // A doubled quote inside is an escaped quote; the loop just reopens the string.
      const close = sql.indexOf(c, i + 1);
      i = close < 0 ? sql.length : close + 1;
    } else if (c === "[") {
      const close = sql.indexOf("]", i + 1);
      i = close < 0 ? sql.length : close + 1;
    } else if (c === "$") {
      // PostgreSQL dollar quoting: $$…$$ or $tag$…$tag$.
      const tag = sql.slice(i).match(/^\$[A-Za-z_]*\$/)?.[0];
      if (tag) {
        const close = sql.indexOf(tag, i + tag.length);
        i = close < 0 ? sql.length : close + tag.length;
      } else i++;
    } else if (c === ";") {
      i++;
      push(i);
    } else i++;
  }
  push(sql.length);
  return out;
}

/** `sql` without the blank space and comments it starts with. */
function stripLeading(sql: string): string {
  let s = sql.trimStart();
  for (let prev = ""; prev !== s; ) {
    prev = s;
    s = s.replace(/^--[^\n]*(\n|$)/, "").replace(/^\/\*[\s\S]*?\*\//, "").trimStart();
  }
  return s;
}

/** Strips comments and blank space from the start, and a closing `;` from the end. */
function core(sql: string): string {
  return stripLeading(sql).replace(/;\s*$/, "").trim();
}

/** Brackets and quotes all close inside `sql`. */
function balanced(sql: string): boolean {
  let depth = 0;
  // Blank out comments and strings, so brackets inside them don't count.
  const plain = sql
    .replace(/--[^\n]*/g, "")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/'(?:[^']|'')*'|"[^"]*"|`[^`]*`/g, "''");
  if (/['"`]/.test(plain.replace(/''/g, ""))) return false;
  for (const ch of plain) {
    if (ch === "(") depth++;
    else if (ch === ")" && --depth < 0) return false;
  }
  return depth === 0;
}

export interface Resolved {
  sql: string;
  /** Where in the editor that SQL is. */
  range: Range;
  /** The selection cut a statement, so whole statements run instead. */
  expanded: boolean;
}

/**
 * What to run for a selection of `sql` from `sel.start` to `sel.end`:
 * - whole statements, as selected;
 * - a complete subquery picked on purpose: a SELECT or WITH in its own brackets, as selected;
 * - anything else: every statement the selection touches, in full.
 * Null when the selection is empty or blank.
 */
export function resolveSelection(sql: string, sel: Range): Resolved | null {
  const text = sql.slice(sel.start, sel.end);
  if (!text.trim()) return null;
  const lead = text.length - text.trimStart().length;
  const trimmed = { start: sel.start + lead, end: sel.start + text.trimEnd().length };

  const touched = statementRanges(sql).filter((r) => r.start < trimmed.end && trimmed.start < r.end);
  if (touched.length === 0) return null;
  const whole = { start: touched[0].start, end: touched[touched.length - 1].end };

  // Each touched statement is fully selected (comments and the closing `;` may be left out).
  const coversAll = touched.every((r) => {
    const outside = sql.slice(r.start, Math.max(r.start, trimmed.start)) + sql.slice(Math.min(r.end, trimmed.end), r.end);
    return core(outside) === "";
  });
  if (coversAll) return { sql: text.trim(), range: trimmed, expanded: false };

  const inner = core(text);
  const before = sql.slice(0, trimmed.start).trimEnd();
  const after = sql.slice(trimmed.end).trimStart();
  const subquery =
    touched.length === 1 && /^(select|with)\b/i.test(inner) && balanced(inner) && before.endsWith("(") && after.startsWith(")");
  if (subquery) return { sql: text.trim(), range: trimmed, expanded: false };

  return { sql: sql.slice(whole.start, whole.end).trim(), range: whole, expanded: true };
}
