import type { DatabaseKind } from "../../api";

export interface KindInfo {
  label: string;
  short: string;
  defaultPort: number | null;
  /** CSS color token for the kind's icon. */
  color: string;
}

export const DATABASE_KINDS: Record<DatabaseKind, KindInfo> = {
  postgres: { label: "PostgreSQL", short: "PG", defaultPort: 5432, color: "var(--color-kind-postgres)" },
  mysql: { label: "MySQL / MariaDB", short: "MY", defaultPort: 3306, color: "var(--color-kind-mysql)" },
  sqlite: { label: "SQLite", short: "SL", defaultPort: null, color: "var(--color-kind-sqlite)" },
};

export const KIND_OPTIONS = (Object.keys(DATABASE_KINDS) as DatabaseKind[]).map((k) => ({
  value: k,
  label: DATABASE_KINDS[k].label,
}));
