import type { ConnectionConfig, ConnectionDraft, Handoff, Target } from "../../api";
import { DATABASE_KINDS } from "../connections/kinds";
import type { NewTab } from "../tabs";

/** What a handoff can do in the window. Nothing here runs SQL or saves a connection. */
export interface HandoffActions {
  connection: (id: string) => ConnectionConfig | null;
  /** Shows the New connection dialog, filled in, with `note` on top. */
  openConnectionDialog: (config: ConnectionConfig, note: string) => void;
  openTab: (target: Target, init: NewTab) => void;
  notify: (text: string) => void;
}

/** "From Claude (MCP)". */
export function handoffNote(source: string | null): string {
  return `From ${source || "an AI app"} (MCP)`;
}

/** The New connection form, filled in from an AI app's draft. The id stays empty: it's new. */
export function draftToConnection(d: ConnectionDraft): ConnectionConfig {
  const kind = DATABASE_KINDS[d.kind];
  return {
    id: "",
    name: d.name,
    kind: d.kind,
    host: d.kind === "sqlite" ? "" : d.host || "localhost",
    port: d.kind === "sqlite" ? null : (d.port ?? kind.defaultPort),
    database: d.database,
    user: d.user,
    file: d.file,
    ssl: d.ssl ?? kind.defaultSsl,
    trustCert: d.trustCert ?? false,
  };
}

/** Shows work handed over by an AI app, for the person to review and act on. */
export function applyHandoff(h: Handoff, a: HandoffActions): void {
  const note = handoffNote(h.source);
  if (h.type === "newConnection") {
    a.openConnectionDialog(draftToConnection(h), note);
    return;
  }
  const c = a.connection(h.connectionId);
  if (!c) {
    a.notify(`${note} asked to open SQL on a connection that isn't saved here.`);
    return;
  }
  a.openTab(
    { connectionId: c.id, database: h.database ?? (c.database.trim() || null) },
    // No autoRun: the person reviews the SQL and clicks Run.
    { title: h.title ?? "", question: h.question ?? "", sql: h.sql, origin: note },
  );
}
