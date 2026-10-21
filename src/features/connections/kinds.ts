import type { DatabaseKind } from "../../api";

export interface KindInfo {
  /** Full name, e.g. in the top bar tooltip. */
  label: string;
  /** Short name for the type picker. */
  option: string;
  /** Two-letter tag in the navigator. */
  short: string;
  defaultPort: number | null;
  /** Encryption on by default (required by Azure SQL). */
  defaultSsl: boolean;
  hostPlaceholder: string;
  /** CSS color token for the kind's icon. */
  color: string;
}

export const DATABASE_KINDS: Record<DatabaseKind, KindInfo> = {
  postgres: {
    label: "PostgreSQL",
    option: "PostgreSQL",
    short: "PG",
    defaultPort: 5432,
    defaultSsl: false,
    hostPlaceholder: "localhost",
    color: "var(--color-kind-postgres)",
  },
  mysql: {
    label: "MySQL / MariaDB",
    option: "MySQL",
    short: "MY",
    defaultPort: 3306,
    defaultSsl: false,
    hostPlaceholder: "localhost",
    color: "var(--color-kind-mysql)",
  },
  mssql: {
    label: "SQL Server / Azure SQL",
    option: "SQL Server",
    short: "MS",
    defaultPort: 1433,
    defaultSsl: true,
    hostPlaceholder: "myserver.database.windows.net or host,port",
    color: "var(--color-kind-mssql)",
  },
  sqlite: {
    label: "SQLite",
    option: "SQLite",
    short: "SL",
    defaultPort: null,
    defaultSsl: false,
    hostPlaceholder: "",
    color: "var(--color-kind-sqlite)",
  },
};

export const KIND_OPTIONS = (Object.keys(DATABASE_KINDS) as DatabaseKind[]).map((k) => ({
  value: k,
  label: DATABASE_KINDS[k].option,
}));
