import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type ConnectionConfig, type Schema } from "../../api";
import { errorText } from "../../lib/errors";

export interface ConnectionsState {
  connections: ConnectionConfig[];
  active: ConnectionConfig | null;
  schema: Schema | null;
  loadingSchema: boolean;
  error: string | null;
  open: (id: string) => void;
  refresh: () => void;
  /** Record a connection saved by the connection dialog, and open it. */
  upsert: (c: ConnectionConfig) => void;
  /** Record a connection deleted by the connection dialog. */
  remove: (id: string) => void;
  dismissError: () => void;
}

/** Saved connections, the open connection and its schema. Opens the first connection on start. */
export function useConnections(initial: ConnectionConfig[]): ConnectionsState {
  const [connections, setConnections] = useState(initial);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [schema, setSchema] = useState<Schema | null>(null);
  const [loadingSchema, setLoadingSchema] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Ignore schema responses for a connection that is no longer the latest one opened.
  const latestRequest = useRef<string | null>(null);

  const load = useCallback(async (id: string, refresh: boolean) => {
    latestRequest.current = id;
    setActiveId(id);
    setLoadingSchema(true);
    setError(null);
    if (!refresh) setSchema(null);
    try {
      const s = refresh ? await commands.refreshSchema(id) : await commands.openConnection(id);
      if (latestRequest.current === id) setSchema(s);
    } catch (e) {
      if (latestRequest.current === id) setError(`Could not connect: ${errorText(e)}`);
    } finally {
      if (latestRequest.current === id) setLoadingSchema(false);
    }
  }, []);

  useEffect(() => {
    if (initial[0]) load(initial[0].id, false);
    // Only on mount: `initial` is the bootstrap snapshot.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const open = useCallback((id: string) => id !== activeId && load(id, false), [activeId, load]);
  const refresh = useCallback(() => activeId && load(activeId, true), [activeId, load]);

  const upsert = useCallback(
    (c: ConnectionConfig) => {
      setConnections((list) => (list.some((x) => x.id === c.id) ? list.map((x) => (x.id === c.id ? c : x)) : [...list, c]));
      // Settings may have changed, so always reconnect.
      load(c.id, false);
    },
    [load],
  );

  const remove = useCallback(
    (id: string) => {
      setConnections((list) => list.filter((x) => x.id !== id));
      if (id === activeId) {
        latestRequest.current = null;
        setActiveId(null);
        setSchema(null);
        setLoadingSchema(false);
      }
    },
    [activeId],
  );

  return {
    connections,
    active: connections.find((c) => c.id === activeId) ?? null,
    schema,
    loadingSchema,
    error,
    open,
    refresh,
    upsert,
    remove,
    dismissError: () => setError(null),
  };
}
