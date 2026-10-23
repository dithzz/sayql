import type { Schema } from "../../api";

/** Example questions for agent mode, shaped by the database: its biggest table, if it has one. */
export function agentStarters(schema: Schema | null): string[] {
  if (!schema?.tables.length) return [];
  const tables = schema.tables.filter((t) => t.kind === "table");
  const biggest = [...tables].sort((a, b) => (b.row_estimate ?? -1) - (a.row_estimate ?? -1))[0];
  const name = biggest?.name.replace(/_/g, " ");
  return [
    "What's in this database, and how do the main tables relate?",
    ...(name ? [`How has the number of ${name} changed month by month?`] : []),
    "Are there duplicates, missing values or orphaned rows I should know about?",
  ];
}
