// A local stand-in for SayQL AI credits, for tests: the real Vercel handlers in site/api/ai,
// served by Node, with a fake Anthropic upstream and a fake credits database (Supabase RPCs).
// Nothing leaves this computer and no real key is used.
//
//   node scripts/ai-credits/harness.mjs [port]
//
// prints {"base": "http://127.0.0.1:<port>/api/ai", "log": "<file>"} on its first line, then
// serves until killed. Every request the fake upstream receives is appended to the log as one
// JSON line (headers and body), so a test can check what reached "Anthropic".
//
// Credit keys: sqlc_test_good (has credit), sqlc_test_broke (on, no credit left);
// any other sqlc_ key is unknown.

import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const GOOD_KEY = "sqlc_test_good";
export const BROKE_KEY = "sqlc_test_broke";
const sha = (k) => crypto.createHash("sha256").update(k).digest("hex");
const ACCOUNTS = {
  [sha(GOOD_KEY)]: { account_id: "acct_good", active: true, balance_micro: 5_000_000 },
  [sha(BROKE_KEY)]: { account_id: "acct_broke", active: true, balance_micro: 0 },
};

const SITE = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../../site");

/** A canned streamed answer, as Anthropic sends it, with pings and usage. */
export function cannedStream(model, text = "Done: nothing to suggest.") {
  const ev = (type, data) => `event: ${type}\ndata: ${JSON.stringify({ type, ...data })}\n\n`;
  return [
    ev("message_start", {
      message: {
        id: "msg_fake",
        type: "message",
        role: "assistant",
        model,
        content: [],
        stop_reason: null,
        usage: { input_tokens: 1200, cache_creation_input_tokens: 300, cache_read_input_tokens: 4000, output_tokens: 1 },
      },
    }),
    ev("ping", {}),
    ev("content_block_start", { index: 0, content_block: { type: "text", text: "" } }),
    ev("content_block_delta", { index: 0, delta: { type: "text_delta", text } }),
    ev("content_block_stop", { index: 0 }),
    ev("message_delta", { delta: { stop_reason: "end_turn", stop_sequence: null }, usage: { output_tokens: 42 } }),
    ev("message_stop", {}),
  ];
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => resolve(Buffer.concat(chunks)));
    req.on("error", reject);
  });
}

function json(res, status, value) {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(value));
}

/** Starts the fake upstream and the credits handlers; resolves to { base, log, settled, close }. */
export async function start({ port = 0, log = path.join(os.tmpdir(), `sayql-credits-${process.pid}-${Date.now()}.jsonl`) } = {}) {
  const settled = [];
  const record = (entry) => fs.appendFileSync(log, JSON.stringify(entry) + "\n");

  // The fake Anthropic API and Supabase, on one server.
  const fake = http.createServer(async (req, res) => {
    const body = await readBody(req);
    const url = new URL(req.url, "http://fake");
    if (url.pathname.startsWith("/rest/v1/rpc/")) {
      const fn = url.pathname.slice("/rest/v1/rpc/".length);
      const args = JSON.parse(body.toString() || "{}");
      const account = ACCOUNTS[args.p_key_sha256];
      if (fn === "ai_account_status") return json(res, 200, account ? [account] : []);
      if (fn === "ai_reserve") {
        return json(res, 200, account && account.active && account.balance_micro >= args.p_amount ? [{ account_id: account.account_id }] : []);
      }
      if (fn === "ai_settle") {
        settled.push(args);
        return json(res, 200, null);
      }
      return json(res, 404, { message: "no such function" });
    }
    let parsed = null;
    try {
      parsed = JSON.parse(body.toString());
    } catch {}
    record({ method: req.method, path: url.pathname + url.search, headers: req.headers, body: parsed });
    if (req.method === "POST" && url.pathname === "/v1/messages/count_tokens") {
      return json(res, 200, { input_tokens: 1234 });
    }
    if (req.method === "POST" && url.pathname === "/v1/messages") {
      // A test asks for an upstream error with metadata.fake_status.
      const status = parsed?.metadata?.fake_status;
      if (status) {
        res.writeHead(status, { "content-type": "application/json", "retry-after": "7", "x-should-retry": "true" });
        return res.end(JSON.stringify({ type: "error", error: { type: "rate_limit_error", message: "fake upstream: slow down" } }));
      }
      if (parsed && parsed.stream) {
        res.writeHead(200, { "content-type": "text/event-stream", "request-id": "req_fake" });
        for (const chunk of cannedStream(parsed.model)) {
          res.write(chunk);
          await new Promise((r) => setTimeout(r, 5));
        }
        return res.end();
      }
      return json(res, 200, {
        id: "msg_fake",
        type: "message",
        role: "assistant",
        model: parsed?.model,
        content: [{ type: "text", text: "Done." }],
        stop_reason: "end_turn",
        usage: { input_tokens: 10, output_tokens: 3 },
      });
    }
    json(res, 404, { type: "error", error: { type: "not_found_error", message: "fake upstream: no such endpoint" } });
  });
  await new Promise((r) => fake.listen(0, "127.0.0.1", r));
  const fakeUrl = `http://127.0.0.1:${fake.address().port}`;

  process.env.SAYQL_AI_UPSTREAM = fakeUrl;
  process.env.SAYQL_AI_ANTHROPIC_KEY = "sk-ant-fake-upstream-key";
  process.env.WAITLIST_SUPABASE_URL = fakeUrl;
  process.env.WAITLIST_SUPABASE_SECRET_KEY = "fake-supabase-secret";
  const load = (f) => import(pathToFileURL(path.join(SITE, "api/ai/v1", f)).href);
  const routes = {
    "/api/ai/v1/messages": await load("messages.js"),
    "/api/ai/v1/models": await load("models.js"),
  };
  try {
    routes["/api/ai/v1/messages/count_tokens"] = await load("messages/count_tokens.js");
  } catch {}

  // The credits service, as Vercel would route it.
  const front = http.createServer(async (req, res) => {
    const url = new URL(req.url, "http://front");
    const mod = routes[url.pathname];
    const handler = mod && mod[req.method];
    if (!handler) {
      record({ front404: true, method: req.method, path: url.pathname + url.search });
      res.writeHead(404, { "content-type": "text/html" });
      return res.end("<h1>404</h1>");
    }
    // What the client sent, credentials left out: whether one came, and how.
    const { authorization, "x-api-key": apiKey, ...seen } = req.headers;
    record({ front: true, method: req.method, path: url.pathname + url.search, auth: authorization ? "bearer" : apiKey ? "x-api-key" : null, headers: seen });
    const body = req.method === "GET" || req.method === "HEAD" ? undefined : await readBody(req);
    const request = new Request(new URL(req.url, `http://${req.headers.host}`), {
      method: req.method,
      headers: Object.entries(req.headers).flatMap(([k, v]) => (Array.isArray(v) ? v.map((x) => [k, x]) : [[k, v]])),
      body,
    });
    const response = await handler(request);
    res.writeHead(response.status, Object.fromEntries(response.headers));
    if (!response.body) return res.end();
    const reader = response.body.getReader();
    req.on("close", () => reader.cancel().catch(() => {}));
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      res.write(value);
    }
    res.end();
  });
  await new Promise((r) => front.listen(port, "127.0.0.1", r));
  const base = `http://127.0.0.1:${front.address().port}/api/ai`;
  return {
    base,
    log,
    settled,
    close: () => Promise.all([new Promise((r) => front.close(r)), new Promise((r) => fake.close(r))]),
  };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const h = await start({ port: Number(process.argv[2] || 0) });
  console.log(JSON.stringify({ base: h.base, log: h.log }));
}
