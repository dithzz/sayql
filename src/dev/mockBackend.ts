// Development only: lets the UI run in a plain browser (`pnpm dev`) with fake data,
// for design work without the Rust backend. Never loaded inside the Tauri app.
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { AgentAccess, AgentReply, AgentStep, AiProvider, Bootstrap, DbNote, QueryResult, Schema, Settings, Skill, Table } from "../api";
import { EXAMPLE_SKILLS, skillError } from "../features/skills";
import { version } from "../../package.json";
import { isReadOnly } from "./readOnly";

/** `?video`: answers and a saved skill for the explainer video (scripts/video), with slower streaming. */
const VIDEO = new URLSearchParams(window.location.search).has("video");

const col = (name: string, data_type: string, primary_key = false) => ({
  name,
  data_type,
  nullable: !primary_key,
  primary_key,
  enum_values: null,
});

const table = (name: string, rows: number, columns: Table["columns"], fks: Table["foreign_keys"] = []): Table => ({
  schema: null,
  name,
  kind: "table",
  columns,
  foreign_keys: fks,
  row_estimate: rows,
});

const schema: Schema = {
  dialect: "SQLite",
  tables: [
    table("categories", 8, [col("id", "integer", true), col("name", "text")]),
    table("customers", 500, [col("id", "integer", true), col("first_name", "text"), col("last_name", "text"), col("email", "text"), col("country", "text"), col("signed_up_at", "text")]),
    table("order_items", 7500, [col("id", "integer", true), col("order_id", "integer"), col("product_id", "integer"), col("quantity", "integer"), col("unit_price", "real")], [
      { column: "order_id", ref_table: "orders", ref_column: "id" },
      { column: "product_id", ref_table: "products", ref_column: "id" },
    ]),
    table("orders", 3000, [col("id", "integer", true), col("customer_id", "integer"), col("status", "text"), col("ordered_at", "text"), col("shipped_at", "text")], [
      { column: "customer_id", ref_table: "customers", ref_column: "id" },
    ]),
    table("products", 120, [col("id", "integer", true), col("name", "text"), col("category_id", "integer"), col("price", "real"), col("stock", "integer")], [
      { column: "category_id", ref_table: "categories", ref_column: "id" },
    ]),
    { ...table("top_customers", -1, [col("customer_id", "integer"), col("total", "real")]), kind: "view" },
  ],
};

const provider = (id: string, label: string, extra: Partial<AiProvider> = {}): AiProvider => ({
  id,
  label,
  note: "",
  api: "openai",
  key: "required",
  keyPlaceholder: "",
  local: false,
  defaultBaseUrl: `https://api.${id}.example/v1`,
  baseUrl: "",
  typingModel: "",
  expertModel: "",
  hasKey: false,
  ...extra,
});

const settings: Settings = {
  provider: "anthropic",
  typingModel: "claude-haiku-5-5",
  expertModel: "claude-opus-5-5",
  aiReady: true,
  onboarded: false,
  workspaceId: "",
  agentModel: "",
  agent: { maxSteps: 12, maxQueries: 10, querySeconds: 30, runSeconds: 180, maxTokens: 400000, skipPlan: false },
  models: [
    { id: "claude-opus-5-5", label: "Claude Opus 5.5 — most accurate" },
    { id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5 — balanced" },
    { id: "claude-haiku-5-5", label: "Claude Haiku 5.5 — fastest" },
  ],
  providers: [
    provider("anthropic", "Anthropic Claude", {
      api: "anthropic",
      note: "Tuned for SayQL, with prompt caching for fast drafts.",
      keyPlaceholder: "sk-ant-…",
      defaultBaseUrl: "https://api.anthropic.com",
      typingModel: "claude-haiku-5-5",
      expertModel: "claude-opus-5-5",
      hasKey: true,
    }),
    provider("openai", "OpenAI", { note: "GPT models with an OpenAI API key.", keyPlaceholder: "sk-…" }),
    provider("gemini", "Google Gemini", { note: "Gemini models with a Google AI Studio key.", keyPlaceholder: "AIza…" }),
    provider("ollama", "Ollama", {
      note: "Runs on this computer. Nothing leaves your machine.",
      key: "none",
      local: true,
      defaultBaseUrl: "http://localhost:11434/v1",
    }),
    provider("custom", "Other (OpenAI-compatible)", { note: "Any server with an OpenAI-compatible API.", key: "optional", defaultBaseUrl: "" }),
  ],
};

const boot: Bootstrap = {
  connections: [
    { id: "demo", name: "Demo store (SQLite)", kind: "sqlite", host: "", port: null, database: "", user: "", file: "/demo.sqlite", ssl: false, trustCert: false },
    { id: "pg", name: "analytics@prod-replica", kind: "postgres", host: "db.internal", port: 5432, database: "analytics", user: "readonly", file: "", ssl: true, trustCert: false },
    { id: "my", name: "shop@localhost", kind: "mysql", host: "localhost", port: 3306, database: "shop", user: "root", file: "", ssl: false, trustCert: false },
    { id: "ms", name: "sales@azure", kind: "mssql", host: "contoso.database.windows.net", port: 1433, database: "sales", user: "reader", file: "", ssl: true, trustCert: false },
  ],
  settings,
};

const names = ["Ava Smith", "Liam Patel", "Noah Garcia", "Emma Kim", "Olivia Nguyen", "Arjun Müller", "Priya Rossi", "Mateo Silva"];
const result: QueryResult = {
  columns: ["customer_id", "customer", "country", "orders", "total_spend", "last_order_at", "notes"],
  rows: Array.from({ length: 240 }, (_, i) => [
    i + 1,
    names[i % names.length],
    ["USA", "India", "UK", "Germany", "Japan"][i % 5],
    20 - (i % 17),
    Math.round((9800 - i * 31.7) * 100) / 100,
    `2026-10-${String(1 + (i % 20)).padStart(2, "0")} 14:${String(i % 60).padStart(2, "0")}:00`,
    i % 4 === 0 ? null : "repeat buyer",
  ]),
  rowsAffected: null,
  truncated: false,
  elapsedMs: 12,
};

/** What SAMPLE_SQL returns: the same columns and row count the query asks for. */
const topCustomers: QueryResult = {
  columns: ["customer_id", "customer", "country", "orders", "total_spend"],
  rows: [
    [214, "Ava Smith", "USA", 31, 18420.5],
    [87, "Liam Patel", "India", 27, 16935.0],
    [342, "Noah Garcia", "Spain", 26, 15210.75],
    [19, "Emma Kim", "South Korea", 24, 14388.2],
    [156, "Olivia Nguyen", "Australia", 22, 13702.4],
    [401, "Arjun Müller", "Germany", 21, 12950.0],
    [63, "Priya Rossi", "Italy", 20, 12118.65],
    [278, "Mateo Silva", "Brazil", 19, 11640.3],
    [125, "Sofia Haddad", "Canada", 18, 10987.1],
    [330, "Kenji Watanabe", "Japan", 17, 10402.85],
  ],
  rowsAffected: null,
  truncated: false,
  elapsedMs: 9,
};

/** What "… FROM orders" returns, so a script's result tabs differ. */
const orders: QueryResult = {
  columns: ["id", "customer_id", "status", "ordered_at"],
  rows: Array.from({ length: 60 }, (_, i) => [
    3000 - i,
    1 + ((i * 37) % 500),
    ["shipped", "pending", "cancelled", "refunded"][i % 4],
    `2026-10-${String(20 - (i % 20)).padStart(2, "0")} 09:${String(i % 60).padStart(2, "0")}:00`,
  ]),
  rowsAffected: null,
  truncated: false,
  elapsedMs: 7,
};

const europe: QueryResult = {
  ...topCustomers,
  rows: [
    [342, "Noah Garcia", "Spain", 26, 15210.75],
    [401, "Arjun Müller", "Germany", 21, 12950.0],
    [63, "Priya Rossi", "Italy", 20, 12118.65],
    [77, "Oliver Brown", "UK", 18, 10870.4],
    [209, "Chloe Dubois", "France", 17, 10233.9],
    [455, "Lars Johansson", "Sweden", 16, 9874.15],
    [12, "Hannah Schmidt", "Germany", 16, 9520.0],
    [388, "Isabella Conti", "Italy", 15, 9105.6],
    [146, "Jakub Nowak", "Poland", 14, 8790.25],
    [261, "Lucas Bernard", "France", 14, 8512.8],
  ],
};

/** New customers, without personal data: what /pii-safe asks for. */
const newCustomers: QueryResult = {
  columns: ["id", "first_name", "last_name", "country", "signed_up_at"],
  rows: [
    [500, "Mia", "Tanaka", "Japan", "2026-10-21"],
    [499, "Leo", "Fischer", "Germany", "2026-10-20"],
    [498, "Zara", "Ahmed", "UK", "2026-10-18"],
    [497, "Diego", "Ramos", "Mexico", "2026-10-17"],
    [496, "Nora", "Lindqvist", "Sweden", "2026-10-15"],
    [495, "Ethan", "Clarke", "USA", "2026-10-12"],
    [494, "Aisha", "Okafor", "Nigeria", "2026-10-09"],
    [493, "Ravi", "Iyer", "India", "2026-10-06"],
    [492, "Camille", "Moreau", "France", "2026-10-03"],
  ],
  rowsAffected: null,
  truncated: false,
  elapsedMs: 6,
};

const japan: QueryResult = {
  columns: ["first_name", "last_name", "signed_up_at"],
  rows: [
    ["Mia", "Tanaka", "2026-10-21"],
    ["Kenji", "Watanabe", "2026-09-28"],
    ["Yui", "Sato", "2026-09-14"],
    ["Haruto", "Kobayashi", "2026-08-30"],
    ["Aoi", "Yamamoto", "2026-08-02"],
  ],
  rowsAffected: null,
  truncated: false,
  elapsedMs: 5,
};

const SAMPLE_SQL = `SELECT c.id AS customer_id,
       c.first_name || ' ' || c.last_name AS customer,
       c.country,
       COUNT(DISTINCT o.id) AS orders,
       ROUND(SUM(oi.quantity * oi.unit_price), 2) AS total_spend
FROM customers c
JOIN orders o ON o.customer_id = c.id
JOIN order_items oi ON oi.order_id = o.id
WHERE o.status NOT IN ('cancelled', 'refunded')
GROUP BY c.id
ORDER BY total_spend DESC
LIMIT 10;`;

const EUROPE_SQL = SAMPLE_SQL.replace(
  "WHERE o.status NOT IN ('cancelled', 'refunded')",
  "WHERE o.status NOT IN ('cancelled', 'refunded')\n  AND c.country IN ('UK', 'France', 'Germany', 'Spain', 'Italy', 'Sweden', 'Poland')",
);

const NEW_CUSTOMERS_SQL = `SELECT id, first_name, last_name, country, signed_up_at
FROM customers
WHERE signed_up_at >= date('now', 'start of month')
ORDER BY signed_up_at DESC;`;

const JAPAN_BROKEN_SQL = `SELECT first_nme, last_name, signed_up_at
FROM customers
WHERE country = 'Japan'
ORDER BY signed_up_at DESC;`;

const MARK_SHIPPED_SQL = "UPDATE orders\nSET status = 'shipped'\nWHERE id = 1042;";

// Questions containing "broken" get a query the mock database rejects, to exercise Fix.
const BROKEN_SQL = "SELECT first_nme, last_name\nFROM customers\nLIMIT 10;";
const FIXED_SQL = "SELECT first_name, last_name\nFROM customers\nLIMIT 10;";

const EXPLANATION =
  "Lists the 10 customers who spent the most. It adds up the price of every item in their orders, " +
  "leaves out cancelled and refunded orders, and shows each customer's name, country, order count and total spend.";

/**
 * Voice input: a fake download and a fake microphone. `?voice=ready` starts with the model
 * downloaded, `?voice=nomic` and `?voice=denied` show those errors.
 */
const VOICE = new URLSearchParams(window.location.search).get("voice");
const VOICE_BYTES = 81_781_811;
const voice = { ready: VOICE === "ready" || VOICE === "nomic" || VOICE === "denied", downloading: false, cancel: false, meter: 0 };

async function fakeDownload() {
  voice.downloading = true;
  voice.cancel = false;
  for (let received = 0; received < VOICE_BYTES; received += 4_000_000) {
    await new Promise((r) => setTimeout(r, 120));
    if (voice.cancel) {
      voice.downloading = false;
      throw "Download canceled.";
    }
    await emit("voice://progress", { received, total: VOICE_BYTES });
  }
  await emit("voice://progress", { received: VOICE_BYTES, total: VOICE_BYTES });
  voice.downloading = false;
  voice.ready = true;
}

function fakeMic() {
  let t = 0;
  voice.meter = globalThis.setInterval(() => {
    t += 1;
    void emit("voice://level", 0.03 + 0.12 * Math.abs(Math.sin(t / 3)) * Math.random());
  }, 50);
  if (VOICE === "denied") setTimeout(() => void emit("voice://blocked", "SayQL can't use the microphone. Allow it in System Settings → Privacy & Security → Microphone."), 1000);
}

/** Skills saved in this preview session; lost on reload. */
let skills: Skill[] = VIDEO ? EXAMPLE_SKILLS.slice(0, 1) : [];

function mockAnswer(question: string, task: { kind: string; previous?: unknown; sql?: string }, skill: string | null): string {
  if (task.kind === "explain") return EXPLANATION;
  if (task.kind === "fix") return task.sql?.includes("first_nme") ? task.sql.replace(/first_nme/g, "first_name") : FIXED_SQL;
  if (VIDEO) {
    if (skill === "pii-safe") return NEW_CUSTOMERS_SQL;
    if (task.previous && /europe/i.test(question)) return EUROPE_SQL;
    if (/japan/i.test(question)) return JAPAN_BROKEN_SQL;
    if (/shipped/i.test(question)) return MARK_SHIPPED_SQL;
  }
  if (task.kind === "improve") return `-- reviewed: added ordering\n${SAMPLE_SQL}`;
  const instruction = skills.find((s) => s.slug === skill)?.instruction;
  if (instruction) return `-- following /${skill}: ${instruction}\n${SAMPLE_SQL}`;
  return /broken/i.test(question) ? BROKEN_SQL : SAMPLE_SQL;
}

async function streamSql(reqId: number, sql: string) {
  const started = performance.now();
  // The video streams token-sized pieces at a readable pace, after a short wait for the first one.
  const chunks = (VIDEO ? sql.match(/\s*[^\s]{1,6}/g) : sql.match(/[\s\S]{1,18}/g)) ?? [];
  if (VIDEO) await new Promise((r) => setTimeout(r, 380));
  for (const [i, text] of chunks.entries()) {
    await new Promise((r) => setTimeout(r, VIDEO ? 12 + Math.random() * 16 : 25));
    await emit("ai://delta", { reqId, text, firstTokenMs: i === 0 ? Math.round(performance.now() - started) : null });
  }
  await emit("ai://done", { reqId, error: null, totalMs: Math.round(performance.now() - started), cacheReadTokens: 1800 });
}

/** Agent mode in the preview: access per connection, notes, and replies runs wait for. */
const agentAccess = new Map<string, AgentAccess>();
let notes: DbNote[] = [];
const waiting = new Map<string, (r: AgentReply) => void>();
const stopped = new Set<string>();

/** A scripted agent run on the demo store: plan, two steps, an optional Send/Skip pause, a cited answer. */
async function mockAgentRun(runId: string, question: string, access: AgentAccess) {
  const pause = () => new Promise((r) => setTimeout(r, VIDEO ? 900 : 500));
  const reply = () => new Promise<AgentReply>((resolve) => waiting.set(runId, resolve));
  const event = (e: Record<string, unknown>) => emit("agent://event", { runId, ...e });
  const usage = (turns: number, queries: number) =>
    event({ type: "progress", turns, maxTurns: 12, queries, maxQueries: 10, usage: { input: 900 * turns, cached: 4200 * turns, output: 180 * turns }, elapsedMs: turns * 1400 });
  const step = (id: number, patch: Partial<AgentStep>): AgentStep => ({
    id, tool: "run_query", text: "", status: "running", sql: null, rowCount: null, truncated: false, elapsedMs: null,
    columns: [], rows: [], sent: null, sentSummary: null, checks: [], error: null, ...patch,
  });
  await pause();
  await usage(1, 0);
  await event({ type: "plan", steps: ["Count paid orders per week", "Split the last week by category", "Compare with the week before"], waiting: true });
  await reply();
  if (stopped.has(runId)) return event({ type: "done", answer: null, error: null, stopped: true });
  const weekly = "SELECT strftime('%Y-%W', ordered_at) AS week, COUNT(*) AS paid FROM orders WHERE status = 'paid' GROUP BY week ORDER BY week DESC LIMIT 2";
  await event({ type: "step", step: step(1, { text: "Counting paid orders in the last two weeks", sql: weekly }) });
  await pause();
  const rows1 = [["2026-42", 31], ["2026-41", 52]];
  const sent1 =
    access.results === "rows" ? `Step 1: 2 rows.\nColumns: ["week","paid"]\n["2026-42",31]\n["2026-41",52]` : access.results === "counts" ? `Step 1: Summary: {"rowCount":2,"columns":[{"name":"week","type":"text","nulls":0,"distinct":2},{"name":"paid","type":"number","nulls":0,"distinct":2,"min":31,"max":52}]}` : `Step 1: 2 rows. Columns: week, paid. No values are sent on this connection.`;
  const summary1 = { rows: "sent 2 rows", counts: "sent counts and column summaries", off: "sent row count only" }[access.results];
  if (access.askBeforeSend && access.results !== "off") {
    await event({ type: "send", stepId: 1, payload: sent1 });
    const r = await reply();
    if (stopped.has(runId)) return event({ type: "done", answer: null, error: null, stopped: true });
    if (r.type === "skip") {
      await event({ type: "step", step: step(1, { text: "Counting paid orders in the last two weeks", sql: weekly, status: "done", rowCount: 2, elapsedMs: 4, columns: ["week", "paid"], rows: rows1, sent: "Step 1: 2 rows. The person chose not to send this result.", sentSummary: "skipped: sent row count only" }) });
    }
  }
  await event({ type: "step", step: step(1, { text: "Counting paid orders in the last two weeks", sql: weekly, status: "done", rowCount: 2, elapsedMs: 4, columns: ["week", "paid"], rows: rows1, sent: sent1, sentSummary: summary1 }) });
  await usage(2, 1);
  await pause();
  const byCat = "SELECT c.name, COUNT(DISTINCT o.id) AS paid FROM orders o JOIN order_items oi ON oi.order_id = o.id JOIN products p ON p.id = oi.product_id JOIN categories c ON c.id = p.category_id WHERE o.status = 'paid' AND strftime('%Y-%W', o.ordered_at) = '2026-42' GROUP BY c.name ORDER BY paid";
  await event({ type: "step", step: step(2, { text: "Splitting last week's paid orders by category", sql: byCat }) });
  await pause();
  if (stopped.has(runId)) return event({ type: "done", answer: null, error: null, stopped: true });
  await event({ type: "step", step: step(2, { text: "Splitting last week's paid orders by category", sql: byCat, status: "done", rowCount: 3, elapsedMs: 9, columns: ["name", "paid"], rows: [["Electronics", 2], ["Books", 11], ["Toys", 18]], sent: "Step 2: 3 rows.", sentSummary: summary1 }) });
  await usage(3, 2);
  await pause();
  await event({
    type: "done",
    error: null,
    stopped: false,
    answer: {
      text: "Paid orders fell from 52 to 31. Electronics had only 2 paid orders last week, the biggest drop.",
      sentences: [
        { text: "Paid orders fell from 52 to 31.", steps: [1], unverified: [] },
        { text: "Electronics had only 2 paid orders last week, the biggest drop.", steps: [2], unverified: [] },
      ],
      confidence: "medium",
      assumptions: ["A week runs Monday to Sunday."],
      notes: ["orders.status 'paid' = paid but not yet shipped"],
      proposedSql: null,
      proposedChangesData: false,
      cannotAnswer: false,
      unverified: [],
      model: "claude-opus-5-5",
      usage: { input: 2700, cached: 12600, output: 540 },
    },
  });
  void question;
}

export function installMockBackend() {
  mockIPC(
    async (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "init":
          return boot;
        case "open_database":
        case "refresh_schema":
          await new Promise((r) => setTimeout(r, 300));
          return schema;
        case "list_databases":
          return a.id === "demo" ? null : { databases: ["analytics", "master", "sales", "staging"], current: "master" };
        case "run_query":
          if (String(a.sql).includes("first_nme")) return Promise.reject(VIDEO ? "no such column: first_nme" : "Invalid column name 'first_nme'.");
          if (!isReadOnly(String(a.sql), boot.connections.find((c) => c.id === a.id)?.kind)) {
            if (!a.allowWrite) return Promise.reject("WRITE_CONFIRM");
            return { columns: [], rows: [], rowsAffected: 1, truncated: false, elapsedMs: 3 };
          }
          if (/from\s+orders\s*;?\s*$/i.test(String(a.sql))) return orders;
          if (String(a.sql).includes("c.country IN")) return europe;
          if (String(a.sql).includes("start of month")) return newCustomers;
          if (String(a.sql).includes("'Japan'")) return japan;
          return String(a.sql).includes("ORDER BY total_spend DESC") ? topCustomers : result;
        case "generate_sql":
          if (a.skill && !skills.some((s) => s.slug === a.skill)) return Promise.reject(`There's no skill called /${a.skill}. Create it in Settings.`);
          void streamSql(Number(a.reqId), mockAnswer(String(a.question), a.task as { kind: string }, (a.skill as string | null) ?? null));
          return null;
        case "cancel_generation":
        case "cancel_query":
          return null;
        case "start_agent": {
          const access = agentAccess.get(String(a.id));
          if (!access?.enabled) return Promise.reject("Agent mode is off for this connection. Turn it on in its agent settings first.");
          void mockAgentRun(String(a.runId), String(a.question), access);
          return null;
        }
        case "stop_agent":
          stopped.add(String(a.runId));
          waiting.get(String(a.runId))?.({ type: "skip" });
          return null;
        case "reply_agent": {
          const resolve = waiting.get(String(a.runId));
          waiting.delete(String(a.runId));
          resolve?.(a.reply as AgentReply);
          return null;
        }
        case "agent_access":
          return agentAccess.get(String(a.id)) ?? { enabled: false, results: "off", askBeforeSend: true, neverSend: [] };
        case "save_agent_access":
          agentAccess.set(String(a.id), a.access as AgentAccess);
          return a.access;
        case "list_notes":
          return notes.filter((n) => n.connectionId === a.id);
        case "save_note": {
          const note = a.note as DbNote;
          if (/@|‹/.test(note.text)) return Promise.reject("A note describes the database, not the data in it. Remove the value (an email, number or name) from it.");
          const saved = { ...note, id: note.id || crypto.randomUUID(), updatedAt: Date.now() };
          notes = [...notes.filter((n) => n.id !== saved.id), saved];
          return saved;
        }
        case "delete_note":
          notes = notes.filter((n) => n.id !== a.id);
          return null;
        case "save_agent_settings":
          Object.assign(settings, { agentModel: a.agentModel, agent: a.agent });
          return { ...settings };
        case "list_scripts":
          return [];
        case "save_script": {
          const script = a.script as { id: string };
          return { ...script, id: script.id || crypto.randomUUID(), updatedAt: Date.now() };
        }
        case "delete_script":
          return null;
        case "list_skills":
          return skills;
        case "save_skill": {
          const original = (a.original as string | null) ?? null;
          const raw = a.skill as Skill;
          const problem = skillError(raw, skills, original);
          if (problem) return Promise.reject(problem);
          const skill = { slug: raw.slug.trim().replace(/^\//, ""), description: raw.description.trim(), instruction: raw.instruction.trim() };
          const i = skills.findIndex((s) => s.slug === original);
          skills = i < 0 ? [...skills, skill] : skills.map((s, j) => (j === i ? skill : s));
          return skill;
        }
        case "delete_skill":
          skills = skills.filter((s) => s.slug !== a.slug);
          return null;
        case "app_info":
          return { version, os: "macos", arch: "aarch64" };
        case "install_update":
          return Promise.reject("Updates aren't available in the browser preview.");
        case "plugin:opener|open_url":
          window.open(String(a.url), "_blank");
          return null;
        case "check_update":
          return null;
        case "take_handoffs":
          return [];
        case "mcp_setup":
          return { path: "/Applications/SayQL.app/Contents/MacOS/sayql-mcp", exists: true, appimage: false };
        case "test_connection":
          return "Connected. Found 6 tables and views.";
        case "save_connection":
          return { ...(a.config as object), id: (a.config as { id: string }).id || crypto.randomUUID() };
        case "delete_connection":
          boot.connections = boot.connections.filter((c) => c.id !== a.id);
          return null;
        case "save_settings": {
          const id = a.provider as string;
          const providers = settings.providers.map((p) =>
            p.id === id ? { ...p, typingModel: a.typingModel as string, expertModel: a.expertModel as string, baseUrl: a.baseUrl as string, hasKey: p.hasKey || !!a.apiKey } : p,
          );
          Object.assign(settings, {
            provider: id,
            typingModel: a.typingModel,
            expertModel: (a.expertModel as string) || a.typingModel,
            workspaceId: a.workspaceId,
            providers,
          });
          return { ...settings };
        }
        case "finish_onboarding":
          settings.onboarded = true;
          return { ...settings };
        case "voice_status":
          return { ready: voice.ready, downloading: voice.downloading, modelBytes: VOICE_BYTES };
        case "voice_download":
          return fakeDownload();
        case "voice_cancel_download":
          voice.cancel = true;
          return null;
        case "voice_delete_model":
          voice.ready = false;
          return null;
        case "voice_start":
          if (VOICE === "nomic") return Promise.reject("No microphone found.");
          fakeMic();
          return null;
        case "voice_stop":
          clearInterval(voice.meter);
          await new Promise((r) => setTimeout(r, 600));
          return "top ten customers by total spend";
        case "voice_cancel":
          clearInterval(voice.meter);
          return null;
        case "list_models":
          if (a.provider === "anthropic") return settings.models.map((m) => m.id);
          return a.provider === "ollama" ? ["llama3.3:70b", "qwen3:32b", "sqlcoder:15b"] : ["model-small", "model-large"];
        default:
          // Fail loudly so a command added to src/api/commands.ts without a mock here shows up.
          throw new Error(`mockBackend: no mock for command "${cmd}"`);
      }
    },
    { shouldMockEvents: true },
  );
}
