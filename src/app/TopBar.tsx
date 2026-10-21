import { Database, DatabaseZap, Plus, Settings as SettingsIcon } from "lucide-react";
import type { ConnectionConfig, Schema } from "../api";
import { Badge, Button, IconButton, ToolbarDivider } from "../components/ui";
import { DATABASE_KINDS } from "../features/connections";
import { ThemeToggle } from "../features/theme";
import styles from "./Workbench.module.css";

export interface TopBarProps {
  active: ConnectionConfig | null;
  databaseName: string | null;
  schema: Schema | null;
  onNewConnection: () => void;
  onOpenSettings: () => void;
}

export function TopBar({ active, databaseName, schema, onNewConnection, onOpenSettings }: TopBarProps) {
  return (
    <header className={styles.topbar}>
      <div className={styles.brand}>
        <DatabaseZap size={18} className={styles.brandMark} />
        Sayql
      </div>
      <ToolbarDivider />
      <Button size="sm" variant="ghost" icon={Plus} onClick={onNewConnection}>
        New connection
      </Button>
      {active && (
        <div className={styles.activeConnection} title={active.kind === "sqlite" ? active.file : `${active.host}:${active.port}`}>
          <Database size={14} color={DATABASE_KINDS[active.kind].color} />
          <span className={styles.activeName}>{active.name}</span>
          {databaseName && active.kind !== "sqlite" && <span className={styles.activeDatabase}>{databaseName}</span>}
          {schema && <Badge>{schema.dialect}</Badge>}
        </div>
      )}
      <span className={styles.spacer} />
      <ThemeToggle />
      <IconButton icon={SettingsIcon} label="Settings" onClick={onOpenSettings} />
    </header>
  );
}
