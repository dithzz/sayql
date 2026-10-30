// Buying on sayql.dev: /api/checkout and /api/dodo-webhook against a fake Dodo, a fake Resend and a
// fake Supabase on local ports. Nothing leaves this computer and no real key is used.

import crypto from "node:crypto";
import http from "node:http";
import { afterAll, beforeAll, beforeEach, expect, it } from "vitest";

const SECRET = "whsec_" + Buffer.from("test-webhook-secret-32-bytes!!!!").toString("base64");
const SEED = crypto.randomBytes(32).toString("base64");
const sha = (s) => crypto.createHash("sha256").update(s).digest("hex");

// ---- a fake Supabase: just the queries the webhook makes ----
let tables, mails, checkouts, nextId;
function reset() {
  tables = { ai_accounts: [], ai_ledger: [], dodo_events: [], licences: [] };
  mails = [];
  checkouts = [];
  nextId = 1;
}
function matches(row, query) {
  for (const [k, v] of query) {
    if (["select", "order", "limit"].includes(k)) continue;
    const [op, val] = [v.slice(0, v.indexOf(".")), v.slice(v.indexOf(".") + 1)];
    const have = row[k];
    if (op === "eq" && String(have) !== val) return false;
    if (op === "is" && String(have) !== val) return false;
  }
  return true;
}
function handleDb(req, raw, res) {
  const url = new URL(req.url, "http://x");
  const send = (code, v) => res.writeHead(code, { "content-type": "application/json" }).end(JSON.stringify(v));
  if (url.pathname === "/rest/v1/rpc/ai_topup") {
    const b = JSON.parse(raw);
    const a = tables.ai_accounts.find((x) => x.id === b.p_account);
    a.balance_micro += b.p_amount;
    tables.ai_ledger.push({ account_id: a.id, kind: b.p_kind, amount_micro: b.p_amount, note: b.p_note, at: nextId++ });
    return send(200, a.balance_micro);
  }
  const table = url.pathname.replace("/rest/v1/", "");
  const rows = tables[table];
  if (!rows) return send(404, {});
  if (req.method === "GET") {
    let out = rows.filter((r) => matches(r, url.searchParams));
    if (url.searchParams.get("order")?.startsWith("at")) out = out.sort((a, b) => a.at - b.at);
    const limit = Number(url.searchParams.get("limit") || 0);
    return send(200, limit ? out.slice(0, limit) : out);
  }
  if (req.method === "POST") {
    const row = JSON.parse(raw);
    if (table === "dodo_events") {
      if (rows.some((r) => r.id === row.id)) return send(201, []);
      rows.push(row);
      return send(201, [row]);
    }
    const stored = { id: nextId++, balance_micro: 0, active: true, created_at: nextId, ...row };
    rows.push(stored);
    return send(201, [stored]);
  }
  if (req.method === "PATCH") {
    rows.filter((r) => matches(r, url.searchParams)).forEach((r) => Object.assign(r, JSON.parse(raw)));
    return send(204, null);
  }
  if (req.method === "DELETE") {
    tables[table] = rows.filter((r) => !matches(r, url.searchParams));
    return send(204, null);
  }
}

let server, base, checkout, webhook, signLicence;
let failMail = 0;
beforeAll(async () => {
  server = http.createServer((req, res) => {
    let raw = "";
    req.on("data", (c) => (raw += c));
    req.on("end", () => {
      if (req.url.startsWith("/rest/v1/")) return handleDb(req, raw, res);
      if (req.url === "/emails") {
        if (failMail > 0) {
          failMail--;
          return res.writeHead(500).end("{}");
        }
        mails.push(JSON.parse(raw));
        return res.writeHead(200, { "content-type": "application/json" }).end('{"id":"e1"}');
      }
      if (req.url === "/checkouts") {
        expect(req.headers.authorization).toBe("Bearer test_dodo_key");
        checkouts.push(JSON.parse(raw));
        return res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ session_id: "cks_1", checkout_url: "https://test.checkout.dodopayments.com/cks_1" }));
      }
      res.writeHead(404).end();
    });
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  base = `http://127.0.0.1:${server.address().port}`;
  Object.assign(process.env, {
    WAITLIST_SUPABASE_URL: base,
    SUPABASE_KEY_BILLING: "test-db-key",
    RESEND_API_KEY: "test-mail-key",
    RESEND_API_URL: `${base}/emails`,
    DODO_API_KEY: "test_dodo_key",
    DODO_WEBHOOK_SECRET: SECRET,
    DODO_PRODUCTS: JSON.stringify({
      team: { id: "pdt_team", kind: "team", users: 10 },
      "credits-25": { id: "pdt_c25", kind: "credits", usd: 25 },
      "credits-custom": { id: "pdt_cx", kind: "credits", custom: true, min: 5, max: 1000 },
    }),
  });
  // Dodo's API at the fake: the module reads test mode's address, so point fetch there.
  const realFetch = globalThis.fetch;
  globalThis.fetch = (url, init) => realFetch(String(url).replace("https://test.dodopayments.com", base), init);
  ({ POST: checkout } = await import("../../site/api/checkout.js"));
  ({ POST: webhook, signLicence } = await import("../../site/api/dodo-webhook.js"));
});
afterAll(() => server.close());
beforeEach(() => {
  reset();
  delete process.env.LICENCE_SIGNING_KEY;
  failMail = 0;
});

/** A webhook as Dodo signs it. */
function event(type, data, { id = crypto.randomUUID(), secret = SECRET, ts = Math.floor(Date.now() / 1000) } = {}) {
  const body = JSON.stringify({ business_id: "bus_1", type, timestamp: new Date().toISOString(), data });
  const key = Buffer.from(secret.replace(/^whsec_/, ""), "base64");
  const sig = crypto.createHmac("sha256", key).update(`${id}.${ts}.${body}`).digest("base64");
  return webhook(new Request("https://sayql.dev/api/dodo-webhook", {
    method: "POST",
    headers: { "webhook-id": id, "webhook-timestamp": String(ts), "webhook-signature": `v1,${sig}` },
    body,
  }));
}
const customer = { customer_id: "cus_1", email: "Ana@Acme.test", name: "Ana" };
const pack = (payment_id = "pay_1") => ({ payment_id, customer, product_cart: [{ product_id: "pdt_c25", quantity: 1 }], subscription_id: null });
const sub = (next = "2026-12-01") => ({
  subscription_id: "sub_1", product_id: "pdt_team", quantity: 1, customer, next_billing_date: `${next}T00:00:00Z`,
  recurring_pre_tax_amount: 14900, payment_frequency_interval: "Month",
});
const keyIn = (text) => (/sqlc_[A-Za-z0-9_-]+/.exec(text) || [])[0];

it("starts a checkout for a product the site sells, and nothing else", async () => {
  let r = await checkout(new Request("https://sayql.dev/api/checkout", { method: "POST", body: JSON.stringify({ product: "credits-25" }) }));
  expect(r.status).toBe(200);
  expect(await r.json()).toEqual({ url: "https://test.checkout.dodopayments.com/cks_1" });
  expect(checkouts[0].product_cart).toEqual([{ product_id: "pdt_c25", quantity: 1 }]);
  expect(checkouts[0].return_url).toBe("https://sayql.dev/thanks/?product=credits-25");
  r = await checkout(new Request("https://sayql.dev/api/checkout", { method: "POST", body: JSON.stringify({ product: "pdt_c25" }) }));
  expect(r.status).toBe(400);
});

it("takes a custom credit amount in whole dollars within the limits, and credits exactly that", async () => {
  const buy = (body) => checkout(new Request("https://sayql.dev/api/checkout", { method: "POST", body: JSON.stringify(body) }));
  for (const usd of [4, 1001, 12.5, "abc", undefined]) expect((await buy({ product: "credits-custom", usd })).status).toBe(400);
  const r = await buy({ product: "credits-custom", usd: 37 });
  expect(r.status).toBe(200);
  expect(checkouts.at(-1).product_cart).toEqual([{ product_id: "pdt_cx", quantity: 1, amount: 3700 }]);
  expect(checkouts.at(-1).metadata).toMatchObject({ credit_usd: "37" });

  const paid = { payment_id: "pay_cx", customer, product_cart: [{ product_id: "pdt_cx", quantity: 1 }], metadata: { credit_usd: "37" }, subscription_id: null };
  expect((await event("payment.succeeded", paid)).status).toBe(200);
  expect(tables.ai_accounts[0].balance_micro).toBe(37_000_000);
});

it("refuses a webhook with a bad signature or an old timestamp", async () => {
  expect((await event("payment.succeeded", pack(), { secret: "whsec_" + Buffer.from("wrong").toString("base64") })).status).toBe(401);
  expect((await event("payment.succeeded", pack(), { ts: Math.floor(Date.now() / 1000) - 3600 })).status).toBe(401);
  expect(tables.ai_accounts).toEqual([]);
});

it("emails a new credit key at once, and a top-up to the same email adds to it", async () => {
  expect((await event("payment.succeeded", pack("pay_1"))).status).toBe(200);
  const [acct] = tables.ai_accounts;
  expect(acct.contact_email).toBe("ana@acme.test");
  expect(acct.balance_micro).toBe(25_000_000);
  const toBuyer = mails.find((m) => m.to[0] === "ana@acme.test");
  expect(toBuyer.subject).toBe("Your SayQL AI credit key");
  expect(sha(keyIn(toBuyer.text))).toBe(acct.key_sha256);
  expect(mails.some((m) => m.to[0] === "hello@azenvoc.com" && m.subject.includes("$25.00"))).toBe(true);

  mails = [];
  expect((await event("payment.succeeded", pack("pay_2"))).status).toBe(200);
  expect(tables.ai_accounts).toHaveLength(1);
  expect(tables.ai_accounts[0].balance_micro).toBe(50_000_000);
  const topUp = mails.find((m) => m.to[0] === "ana@acme.test");
  expect(topUp.subject).toBe("SayQL AI credit added: $25.00");
  expect(keyIn(topUp.text)).toBeUndefined();
});

it("ignores payments and subscriptions for other products on the same Dodo account", async () => {
  const other = { ...pack("pay_other"), product_cart: [{ product_id: "pdt_someone_else", quantity: 1 }] };
  expect((await event("payment.succeeded", other)).status).toBe(200);
  expect((await event("subscription.active", { ...sub(), product_id: "pdt_someone_else" })).status).toBe(200);
  expect(tables.ai_accounts).toEqual([]);
  expect(mails).toEqual([]);
});

it("handles a repeated webhook once", async () => {
  await event("payment.succeeded", pack(), { id: "msg_same" });
  await event("payment.succeeded", pack(), { id: "msg_same" });
  expect(tables.ai_accounts[0].balance_micro).toBe(25_000_000);
});

it("if the key email fails, Dodo's retry sends a working key without adding the credit twice", async () => {
  failMail = 1;
  expect((await event("payment.succeeded", pack(), { id: "msg_r" })).status).toBe(500);
  expect(tables.dodo_events).toEqual([]);
  expect((await event("payment.succeeded", pack(), { id: "msg_r" })).status).toBe(200);
  const [acct] = tables.ai_accounts;
  expect(acct.balance_micro).toBe(25_000_000);
  const key = keyIn(mails.find((m) => m.to[0] === "ana@acme.test").text);
  expect(sha(key)).toBe(acct.key_sha256);
});

it("without the signing key, a Team purchase sends a receipt and the owner the command to run", async () => {
  expect((await event("subscription.active", sub())).status).toBe(200);
  const toBuyer = mails.find((m) => m.to[0] === "ana@acme.test");
  expect(toBuyer.subject).toBe("SayQL for teams: payment received");
  const toOwner = mails.find((m) => m.to[0] === "hello@azenvoc.com");
  expect(toOwner.text).toContain('--licensee "Ana" --plan team --users 10 --expires 2026-12-15');
  expect(tables.licences).toEqual([]);
});

it("with the signing key, emails a licence to the next billing date plus 14 days, once per period", async () => {
  process.env.LICENCE_SIGNING_KEY = SEED;
  await event("subscription.active", sub("2026-12-01"));
  await event("subscription.renewed", sub("2026-12-01"));
  const keys = mails.filter((m) => m.subject === "Your SayQL for teams licence key");
  expect(keys).toHaveLength(1);
  const token = /([A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)\n/.exec(keys[0].text)[1];
  const [payload, sig] = token.split(".");
  const l = JSON.parse(Buffer.from(payload, "base64url"));
  expect(l).toMatchObject({ v: 1, licensee: "Ana", plan: "team", users: 10, expires: "2026-12-15" });
  // Signed by the key behind the seed.
  const pub = crypto.createPublicKey(crypto.createPrivateKey({
    key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), Buffer.from(SEED, "base64")]),
    format: "der", type: "pkcs8",
  }));
  expect(crypto.verify(null, Buffer.from(payload, "base64url"), pub, Buffer.from(sig, "base64url"))).toBe(true);
  expect(tables.licences[0]).toMatchObject({ licensee: "Ana", users: 10, expires: "2026-12-15", price_usd_month: 149, key_sha256: sha(token) });

  await event("subscription.renewed", sub("2027-01-01"));
  expect(mails.filter((m) => m.subject === "Your SayQL for teams licence key")).toHaveLength(2);
  expect(signLicence).toBeTypeOf("function");
});
