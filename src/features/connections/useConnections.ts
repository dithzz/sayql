import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type ConnectionConfig, type DatabaseList, type Schema, type Target } from "../../api";
import { errorText } from "../../lib/errors";
import { sameTarget } from "./target";

export interface ConnectionsState {
  connections: ConnectionConfig[];
  active: ConnectionConfig | null;
  /** The open database; `null` until a connection is open. */
  target: Target | null;
  /** Name of the open database, for display (the server's default when none was chosen). */
  databaseName: string | null;
  /** The server's databases per connection; `null` for SQLite, absent while loading. */
  databases: Record<string, DatabaseList | null>;
  schema: Schema | null;
  loadingSchema: boolean;
  error: string | null;
  open: (connectionId: string) => void;
  openDatabase: (database: string) => void;
  refresh: () => void;
  /** Record a connection saved by the connection dialog, and open it. */
  upsert: (c: ConnectionConfig) => void;
  /** Record a connection deleted by the connection dialog. */
  remove: (id: string) => void;
  dismissError: () => void;
}

/** Saved connections, the open database and its schema. Opens the first connection on start. */
export function useConnections(initial: ConnectionConfig[]): ConnectionsState {
  const [connections, setConnections] = useState(initial);
  const [target, setTarget] = useState<Target | null>(null);
  const [databases, setDatabases] = useState<Record<string, DatabaseList | null>>({});
  const [schema, setSchema] = useState<Schema | null>(null);
  const [loadingSchema, setLoadingSchema] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Ignore responses for a target that is no longer the latest one opened.
  const latest = useRef<Target | null>(null);

  const load = useCallback(async (t: Target, refresh: boolean) => {
    latest.current = t;
    setTarget(t);
    setLoadingSchema(true);
    setError(null);
    if (!refresh) setSchema(null);
    try {
      const s = refresh ? await commands.refreshSchema(t) : await commands.openDatabase(t);
      if (sameTarget(latest.current, t)) setSchema(s);
    } catch (e) {
      if (sameTarget(latest.current, t)) setError(`Could not open the database: ${errorText(e)}`);
    } finally {
      if (sameTarget(latest.current, t)) setLoadingSchema(false);
    }
  }, []);

  const loadDatabaseList = useCallback(async (connectionId: string) => {
    try {
      const list = await commands.listDatabases(connectionId);
      setDatabases((all) => ({ ...all, [connectionId]: list }));
    } catch {
      // Listing needs extra permissions on some servers; the open database still works.
      setDatabases((all) => ({ ...all, [connectionId]: null }));
    }
  }, []);

  /** Opens a connection on the database from its settings (or the server default). */
  const openConnection = useCallback(
    (c: ConnectionConfig) => {
      load({ connectionId: c.id, database: c.database.trim() || null }, false);
      if (c.kind !== "sqlite") loadDatabaseList(c.id);
    },
    [load, loadDatabaseList],
  );

  useEffect(() => {
    if (initial[0]) openConnection(initial[0]);
    // Only on mount: `initial` is the bootstrap snapshot.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const open = useCallback(
    (id: string) => {
      const c = connections.find((x) => x.id === id);
      if (c && id !== target?.connectionId) openConnection(c);
    },
    [connections, openConnection, target],
  );

  const list = target ? databases[target.connectionId] : undefined;
  const databaseName = target ? (target.database ?? list?.current ?? null) : null;

  const openDatabase = useCallback(
    (database: string) => {
      if (!target || database === databaseName) return;
      load({ connectionId: target.connectionId, database }, false);
    },
    [databaseName, load, target],
  );

  const refresh = useCallback(() => {
    if (!target) return;
    load(target, true);
    loadDatabaseList(target.connectionId);
  }, [load, loadDatabaseList, target]);

  const upsert = useCallback(
    (c: ConnectionConfig) => {
      setConnections((all) => (all.some((x) => x.id === c.id) ? all.map((x) => (x.id === c.id ? c : x)) : [...all, c]));
      // The details may have changed, so always reconnect.
      openConnection(c);
    },
    [openConnection],
  );

  const remove = useCallback(
    (id: string) => {
      setConnections((all) => all.filter((x) => x.id !== id));
      if (id === target?.connectionId) {
        latest.current = null;
        setTarget(null);
        setSchema(null);
        setLoadingSchema(false);
      }
    },
    [target],
  );

  return {
    connections,
    active: connections.find((c) => c.id === target?.connectionId) ?? null,
    target,
    databaseName,
    databases,
    schema,
    loadingSchema,
    error,
    open,
    openDatabase,
    refresh,
    upsert,
    remove,
    dismissError: () => setError(null),
  };
}
