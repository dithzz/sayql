import { useMemo, useState } from "react";
import { qualifiedName, type ConnConfig, type Schema, type Table } from "../api";

const KIND_LABEL = { postgres: "PG", mysql: "MY", sqlite: "SL" } as const;

interface Props {
  connections: ConnConfig[];
  activeId: string | null;
  schema: Schema | null;
  loading: boolean;
  onOpen: (id: string) => void;
  onAdd: () => void;
  onEdit: (c: ConnConfig) => void;
  onRefresh: () => void;
  onPreviewTable: (t: Table) => void;
  onSettings: () => void;
}

export function Sidebar(p: Props) {
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState<Set<string>>(new Set());

  const tables = useMemo(() => {
    const f = filter.trim().toLowerCase();
    if (!p.schema) return [];
    if (!f) return p.schema.tables;
    return p.schema.tables.filter(
      (t) => t.name.toLowerCase().includes(f) || t.columns.some((c) => c.name.toLowerCase().includes(f)),
    );
  }, [p.schema, filter]);

  const toggle = (key: string) =>
    setOpen((s) => {
      const n = new Set(s);
      n.has(key) ? n.delete(key) : n.add(key);
      return n;
    });

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="logo">◆</span> AskDB
        <button className="ghost icon" title="Settings" onClick={p.onSettings}>
          ⚙
        </button>
      </div>

      <div className="section-head">
        Connections
        <button className="ghost icon" title="New connection" onClick={p.onAdd}>
          ＋
        </button>
      </div>
      <ul className="conn-list">
        {p.connections.map((c) => (
          <li key={c.id} className={c.id === p.activeId ? "active" : ""} onClick={() => p.onOpen(c.id)}>
            <span className={`kind kind-${c.kind}`}>{KIND_LABEL[c.kind]}</span>
            <span className="conn-name">{c.name}</span>
            <button
              className="ghost icon edit"
              title="Edit connection"
              onClick={(e) => {
                e.stopPropagation();
                p.onEdit(c);
              }}
            >
              ✎
            </button>
          </li>
        ))}
        {p.connections.length === 0 && <li className="muted">No connections yet</li>}
      </ul>

      {p.activeId && (
        <>
          <div className="section-head">
            Schema {p.schema && <span className="count">{p.schema.tables.length}</span>}
            <button className="ghost icon" title="Reload schema" onClick={p.onRefresh} disabled={p.loading}>
              ↻
            </button>
          </div>
          <input
            className="filter"
            placeholder="Filter tables and columns"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <div className="tree">
            {p.loading && <div className="muted pad">Reading schema…</div>}
            {tables.map((t) => {
              const key = qualifiedName(t);
              const fks = new Map(t.foreign_keys.map((f) => [f.column, f]));
              return (
                <div key={key}>
                  <div
                    className="tree-table"
                    onClick={() => toggle(key)}
                    onDoubleClick={() => p.onPreviewTable(t)}
                    title="Double-click to preview rows"
                  >
                    <span className="caret">{open.has(key) || filter ? "▾" : "▸"}</span>
                    <span className={t.kind === "view" ? "view" : ""}>{key}</span>
                    {t.row_estimate != null && t.row_estimate >= 0 && (
                      <span className="rows">{t.row_estimate.toLocaleString()}</span>
                    )}
                  </div>
                  {(open.has(key) || filter) &&
                    t.columns.map((c) => {
                      const fk = fks.get(c.name);
                      return (
                        <div className="tree-col" key={c.name} title={fk ? `→ ${fk.ref_table}.${fk.ref_column}` : ""}>
                          <span className="col-icon">{c.primary_key ? "🔑" : fk ? "↗" : "·"}</span>
                          <span>{c.name}</span>
                          <span className="type">{c.data_type}</span>
                        </div>
                      );
                    })}
                </div>
              );
            })}
          </div>
        </>
      )}
    </aside>
  );
}
