// The owner portal's "New key" for a SayQL AI credit account: the real handler (site/api/owner.js)
// against a fake Supabase on a local port. Nothing leaves this computer.

import crypto from "node:crypto";
import http from "node:http";
import { afterAll, beforeAll, expect, it } from "vitest";

const SECRET = "test-session-secret";
const sha = (k) => crypto.createHash("sha256").update(k).digest("hex");
const accounts = { 7: { id: 7, name: "Acme", key_sha256: sha("sqlc_old"), balance_micro: 12_000_000, active: true } };

let server, handler;
beforeAll(async () => {
  // The fake database: PATCH ai_accounts?id=eq.N sets the fields sent and returns the row.
  server = http.createServer((req, res) => {
    const m = /^\/rest\/v1\/ai_accounts\?id=eq\.(\d+)$/.exec(req.url);
    let raw = "";
    req.on("data", (c) => (raw += c));
    req.on("end", () => {
      if (req.method !== "PATCH" || !m) return res.writeHead(404).end("{}");
      const row = accounts[m[1]];
      if (row) Object.assign(row, JSON.parse(raw));
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(row ? [row] : []));
    });
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  process.env.WAITLIST_SUPABASE_URL = `http://127.0.0.1:${server.address().port}`;
  process.env.SUPABASE_KEY_OWNER = "test-db-key";
  process.env.OWNER_PASSWORD_HASH = "unused";
  process.env.OWNER_SESSION_SECRET = SECRET;
  ({ default: handler } = await import("../../site/api/owner.js"));
});
afterAll(() => server.close());

function signedInCookie() {
  const value = `v1.${Math.floor(Date.now() / 1000) + 600}`;
  const mac = crypto.createHmac("sha256", SECRET).update(value).digest("base64url");
  return `sayql_owner=${value}.${mac}`;
}

/** Calls the handler as Vercel would, and returns [status, json]. */
function call(action, body, cookie = signedInCookie()) {
  return new Promise((resolve) => {
    let status = 200;
    const res = {
      setHeader() {},
      status(s) {
        status = s;
        return res;
      },
      json(v) {
        resolve([status, v]);
        return res;
      },
    };
    handler({ method: "POST", query: { action }, headers: { cookie, "x-requested-with": "sayql-owner" }, body }, res);
  });
}

it("gives an account a new key and keeps its balance; the old key stops matching", async () => {
  const [status, r] = await call("ai-rekey", { account_id: 7 });
  expect(status).toBe(200);
  expect(r.key).toMatch(/^sqlc_[A-Za-z0-9_-]{32}$/);
  expect(accounts[7].key_sha256).toBe(sha(r.key));
  expect(accounts[7].key_sha256).not.toBe(sha("sqlc_old"));
  expect(accounts[7].balance_micro).toBe(12_000_000);
  expect(accounts[7].active).toBe(true);
});

it("refuses an unknown account, a missing one, and anyone not signed in", async () => {
  expect((await call("ai-rekey", { account_id: 99 }))[0]).toBe(404);
  expect((await call("ai-rekey", {}))[0]).toBe(400);
  expect((await call("ai-rekey", { account_id: 7 }, "sayql_owner=v1.1.bad"))[0]).toBe(401);
});
