import { useMemo, useState } from "react";
import { Database, Eye, Folder, KeyRound, Link2, Pencil, Plus, RefreshCw, Search, Server, Table2, Type } from "lucide-react";
import type { ConnectionConfig, DatabaseList, Schema, Table } from "../../api";
import { IconButton, Panel, Spinner, TextInput, TreeRow } from "../../components/ui";
import { cx } from "../../lib/cx";
import { DATABASE_KINDS } from "../connections/kinds";
import { qualifiedName } from "../schema/sql";
import styles from "./DatabaseNavigator.module.css";

export interface DatabaseNavigatorProps {
  connections: ConnectionConfig[];
  activeId: string | null;
  /** The server's databases per connection; `null` for SQLite, absent while loading. */
  databases: Record<string, DatabaseList | null>;
  /** The open database on the active connection. */
  databaseName: string | null;
  schema: Schema | null;
  loading: boolean;
  onOpen: (id: string) => void;
  onOpenDatabase: (database: string) => void;
  onAdd: () => void;
  onEdit: (c: ConnectionConfig) => void;
  onRefresh: () => void;
  /** Double-click on a table. */
  onPreviewTable: (t: Table) => void;
}

/**
 * Connection → Databases → database → Tables/Views → table → columns, like DBeaver's
 * Database Navigator. SQLite skips the Databases level: a file is one database.
 */
export function DatabaseNavigator(p: DatabaseNavigatorProps) {
  const [filter, setFilter] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [expandedTables, setExpandedTables] = useState<Set<string>>(new Set());
  const query = filter.trim().toLowerCase();

  const { tables, views } = useMemo(() => {
    const all = (p.schema?.tables ?? []).filter(
      (t) => !query || t.name.toLowerCase().includes(query) || t.columns.some((c) => c.name.toLowerCase().includes(query)),
    );
    return { tables: all.filter((t) => t.kind === "table"), views: all.filter((t) => t.kind === "view") };
  }, [p.schema, query]);

  const toggle = (set: Set<string>, key: string) => {
    const next = new Set(set);
    next.has(key) ? next.delete(key) : next.add(key);
    return next;
  };

  const renderTable = (t: Table, depth: number) => {
    const key = qualifiedName(t);
    const open = !!query || expandedTables.has(key);
    const fks = new Map(t.foreign_keys.map((f) => [f.column, f]));
    return (
      <div key={key} role="group">
        <TreeRow
          depth={depth}
          expanded={open}
          icon={t.kind === "view" ? <Eye size={14} /> : <Table2 size={14} />}
          label={<span className={cx(styles.mono, t.kind === "view" && styles.view)}>{key}</span>}
          meta={t.row_estimate != null && t.row_estimate >= 0 ? t.row_estimate.toLocaleString() : undefined}
          title="Double-click to preview rows"
          onClick={() => setExpandedTables((s) => toggle(s, key))}
          onDoubleClick={() => p.onPreviewTable(t)}
        />
        {open &&
          t.columns.map((c) => {
            const fk = fks.get(c.name);
            const icon = c.primary_key ? (
              <KeyRound size={13} className={styles.pk} />
            ) : fk ? (
              <Link2 size={13} className={styles.fk} />
            ) : (
              <Type size={13} />
            );
            return (
              <TreeRow
                key={c.name}
                depth={depth + 1}
                icon={icon}
                label={<span className={styles.mono}>{c.name}</span>}
                meta={c.data_type}
                title={fk ? `References ${fk.ref_table}.${fk.ref_column}` : c.nullable ? "Nullable" : "Not null"}
              />
            );
          })}
      </div>
    );
  };

  const renderFolder = (id: string, label: string, items: Table[], depth: number) => {
    if (items.length === 0) return null;
    const open = !!query || !collapsed.has(id);
    return (
      <div role="group">
        <TreeRow
          depth={depth}
          expanded={open}
          icon={<Folder size={14} />}
          label={label}
          meta={items.length}
          onClick={() => setCollapsed((s) => toggle(s, id))}
        />
        {open && items.map((t) => renderTable(t, depth + 1))}
      </div>
    );
  };

  /** Tables and Views of the open schema, starting at `depth`. */
  const renderSchema = (key: string, depth: number) =>
    p.schema && (
      <>
        {renderFolder(`${key}:tables`, "Tables", tables, depth)}
        {renderFolder(`${key}:views`, "Views", views, depth)}
        {tables.length + views.length === 0 && (
          <TreeRow depth={depth} label={<span className={styles.message}>{query ? "No matches" : "No tables"}</span>} />
        )}
      </>
    );

  const renderDatabases = (c: ConnectionConfig, list: DatabaseList) => {
    const folder = `${c.id}:databases`;
    const open = !collapsed.has(folder);
    return (
      <div role="group">
        <TreeRow
          depth={1}
          expanded={open}
          icon={<Folder size={14} />}
          label="Databases"
          meta={list.databases.length}
          onClick={() => setCollapsed((s) => toggle(s, folder))}
        />
        {open &&
          list.databases.map((name) => {
            const current = name === p.databaseName;
            return (
              <div key={name} role="group">
                <TreeRow
                  depth={2}
                  expanded={current}
                  selected={current}
                  icon={<Database size={14} />}
                  label={<span className={cx(current && styles.connLabel)}>{name}</span>}
                  meta={current && p.loading ? <Spinner size={12} /> : undefined}
                  title={current ? undefined : "Open this database"}
                  onClick={() => p.onOpenDatabase(name)}
                />
                {current && renderSchema(`${c.id}/${name}`, 3)}
              </div>
            );
          })}
      </div>
    );
  };

  return (
    <Panel
      title="Database Navigator"
      icon={Server}
      actions={
        <>
          <IconButton icon={Plus} label="New connection" size="sm" onClick={p.onAdd} />
          <IconButton icon={RefreshCw} label="Reload schema" size="sm" onClick={p.onRefresh} disabled={!p.activeId || p.loading} />
        </>
      }
    >
      <div className={styles.filter}>
        <Search size={13} className={styles.filterIcon} />
        <TextInput
          className={styles.filterInput}
          placeholder="Filter tables and columns"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>
      <div className={styles.tree} role="tree" aria-label="Database objects">
        {p.connections.map((c) => {
          const active = c.id === p.activeId;
          const list = p.databases[c.id];
          return (
            <div key={c.id} role="group">
              <TreeRow
                depth={0}
                expanded={active}
                selected={active}
                icon={
                  c.kind === "sqlite" ? (
                    <Database size={14} color={DATABASE_KINDS[c.kind].color} />
                  ) : (
                    <Server size={14} color={DATABASE_KINDS[c.kind].color} />
                  )
                }
                label={<span className={styles.connLabel}>{c.name}</span>}
                meta={active && p.loading && !list ? <Spinner size={12} /> : DATABASE_KINDS[c.kind].short}
                actions={<IconButton icon={Pencil} label="Edit connection" size="sm" onClick={() => p.onEdit(c)} />}
                onClick={() => p.onOpen(c.id)}
              />
              {/* Servers list their databases; SQLite (or a server that won't list them) shows the schema directly. */}
              {active && (list ? renderDatabases(c, list) : renderSchema(c.id, 1))}
            </div>
          );
        })}
        {p.connections.length === 0 && <TreeRow depth={0} label={<span className={styles.message}>No connections yet</span>} />}
      </div>
    </Panel>
  );
}
