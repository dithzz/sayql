// POST /api/dodo-webhook: Dodo Payments tells sayql.dev about payments, and the buyer gets their key by
// email within seconds (Vercel function).
//
//   payment.succeeded (a one-time AI credit pack): creates a credit account for the buyer's email and
//     emails its key, or tops up the account that email already has and says so.
//   subscription.active / subscription.renewed (Team): signs a licence to the next billing date plus
//     14 days and emails it, when LICENCE_SIGNING_KEY is set. Without it, the buyer gets a receipt and
//     the owner gets the `sayql-license sign` command to run by hand.
//
// Each webhook is handled once (table dodo_events); a failure releases it so Dodo's retry runs again.
// Keys are emailed and never stored: only their SHA-256. Environment: see _lib/dodo.js and _lib/mail.js,
// plus LICENCE_SIGNING_KEY (optional): the base64 32-byte Ed25519 seed that `sayql-license` signs with.

import crypto from "node:crypto";
import { productById, verifyWebhook } from "./_lib/dodo.js";
import { OWNER, send } from "./_lib/mail.js";
import { headers, rpc, store } from "./_lib/supabase.js";

export const config = { runtime: "nodejs" };

const GRACE_DAYS = 14;
const SETUP_TEAM = `On your team server's admin page, open AI and set:
  Provider: Anthropic
  Base URL: https://sayql.dev/api/ai
  API key:  the key above

For yourself only: in SayQL, Settings → AI provider → SayQL AI credits, and paste the key.`;

export async function POST(request) {
  const raw = await request.text();
  if (!verifyWebhook(raw, request.headers, process.env.DODO_WEBHOOK_SECRET)) {
    return json(401, { error: "Bad signature." });
  }
  const db = store("BILLING");
  if (!db) return json(503, { error: "Not set up." });
  let event;
  try {
    event = JSON.parse(raw);
  } catch {
    return json(400, { error: "Not JSON." });
  }
  const id = request.headers.get("webhook-id");
  if (!(await claim(db, id, event.type))) return json(200, { ok: true, duplicate: true });
  try {
    await handle(db, event);
    return json(200, { ok: true });
  } catch (e) {
    console.error("dodo-webhook:", event.type, e.message);
    await release(db, id).catch(() => {});
    return json(500, { error: "Failed; retry." });
  }
}

async function handle(db, event) {
  const data = event.data || {};
  if (event.type === "payment.succeeded" && !data.subscription_id) {
    const packs = (data.product_cart || [])
      .map((i) => ({ p: productById(i.product_id), qty: Number(i.quantity) || 1 }))
      .filter((i) => i.p && i.p.kind === "credits");
    // The Dodo account sells other products too; their payments aren't SayQL's to handle.
    if (!packs.length) return;
    // A custom amount is what /api/checkout put in the order's metadata (set by us, not the buyer).
    const custom = Number(data.metadata?.credit_usd);
    const usd = packs.reduce((sum, i) => sum + (i.p.custom ? custom : Number(i.p.usd)) * i.qty, 0);
    if (!(usd > 0)) {
      return notifyOwner("Credit payment without an amount", `Payment ${data.payment_id} from ${data.customer?.email} has no credit_usd. Add the credit by hand in the owner portal.`);
    }
    return credits(db, data.customer, usd, data.payment_id);
  }
  if (event.type === "subscription.active" || event.type === "subscription.renewed") {
    const p = productById(data.product_id);
    if (!p || p.kind !== "team") return;
    return team(db, data, p);
  }
}

// ---- AI credits ----

async function credits(db, customer, usd, paymentId) {
  const email = String(customer?.email || "").trim().toLowerCase();
  if (!email) throw new Error("payment without an email");
  const note = `dodo ${paymentId}`;
  // A retry after a failed email: the credit is already there, so don't add it twice.
  const [done] = await select(db, `ai_ledger?note=eq.${enc(note)}&select=account_id&limit=1`);
  let account, key = null;
  if (done) {
    [account] = await select(db, `ai_accounts?id=eq.${done.account_id}&select=id,name,balance_micro`);
    const [first] = await select(db, `ai_ledger?account_id=eq.${account.id}&select=note&order=at.asc&limit=1`);
    // This payment opened the account, and its key never arrived: issue a fresh one.
    if (first && first.note === note) key = await rekey(db, account.id);
  } else {
    [account] = await select(db, `ai_accounts?contact_email=eq.${enc(email)}&active=is.true&select=id,name,balance_micro&order=created_at.asc&limit=1`);
    if (!account) {
      key = newKey();
      [account] = await insert(db, "ai_accounts", { name: customer.name || email, contact_email: email, key_sha256: sha256(key) });
    }
    account.balance_micro = await rpc(db, "ai_topup", { p_account: account.id, p_amount: Math.round(usd * 1e6), p_kind: "topup", p_note: note });
  }
  const balance = money(Number(account.balance_micro) / 1e6);
  if (key) {
    await send({
      to: email,
      subject: "Your SayQL AI credit key",
      text: `Thanks for buying SayQL AI credits. Your balance is ${balance}.

Your credit key (keep it private; anyone with it can use your credit):

${key}

${SETUP_TEAM}

Requests are charged at Anthropic's API prices plus 20%. SayQL shows what's left and warns when it's low.
To buy more, use the same email on sayql.dev and it's added to this key.

SayQL, a product of Azenvoc · hello@azenvoc.com`,
    });
  } else {
    await send({
      to: email,
      subject: `SayQL AI credit added: ${money(usd)}`,
      text: `${money(usd)} of SayQL AI credit was added to your key. Your balance is now ${balance}.
Your key stays the same; nothing to change in SayQL.

SayQL, a product of Azenvoc · hello@azenvoc.com`,
    });
  }
  await notifyOwner(`Sale: ${money(usd)} AI credits`, `${customer.name || ""} <${email}>, payment ${paymentId}. ${key ? "New account" : "Top-up"} #${account.id}, balance ${balance}.`);
}

async function rekey(db, accountId) {
  const key = newKey();
  const r = await fetch(`${db.url}/rest/v1/ai_accounts?id=eq.${accountId}`, {
    method: "PATCH",
    headers: headers(db, { "Content-Type": "application/json", Prefer: "return=minimal" }),
    body: JSON.stringify({ key_sha256: sha256(key) }),
  });
  if (!r.ok) throw new Error(`ai_accounts answered ${r.status}`);
  return key;
}

// ---- Team licences ----

async function team(db, sub, product) {
  const email = String(sub.customer?.email || "").trim().toLowerCase();
  const period = String(sub.next_billing_date || "").slice(0, 10);
  if (!email || !/^\d{4}-\d{2}-\d{2}$/.test(period)) throw new Error("subscription without an email or billing date");
  // subscription.active and .renewed can both arrive for one period: one licence per period.
  const marker = `licence ${sub.subscription_id} ${period}`;
  if (!(await claim(db, marker, "licence"))) return;
  try {
    const users = (Number(product.users) || 10) * (Number(sub.quantity) || 1);
    const issued = new Date().toISOString().slice(0, 10);
    const expires = addDays(period, GRACE_DAYS);
    const licensee = (sub.customer.name || email).slice(0, 200);
    const monthly = Number(sub.recurring_pre_tax_amount) / 100 / (sub.payment_frequency_interval === "Year" ? 12 : 1);
    const seed = process.env.LICENCE_SIGNING_KEY;
    if (seed) {
      const token = signLicence({ v: 1, licensee, plan: "team", users, databases: 0, issued, expires }, seed);
      await insert(db, "licences", {
        licensee, contact_email: email, plan: "team", users, issued, expires,
        price_usd_month: Number.isFinite(monthly) ? monthly : null, key_sha256: sha256(token), notes: `dodo ${sub.subscription_id}`,
      });
      await send({
        to: email,
        subject: "Your SayQL for teams licence key",
        text: `Thanks for choosing SayQL for teams. Your licence: Team, up to ${users} people, until ${expires}.

Your licence key:

${token}

Paste it on your server's admin page: Overview → Licence key → Save key. It takes effect at once, with no restart.
Each renewal emails a new key a few days before this one ends.

SayQL, a product of Azenvoc · hello@azenvoc.com`,
      });
      await notifyOwner(`Sale: Team for ${licensee}`, `<${email}>, ${users} people, until ${expires}, subscription ${sub.subscription_id}. Licence signed and sent.`);
    } else {
      await send({
        to: email,
        subject: "SayQL for teams: payment received",
        text: `Thanks for choosing SayQL for teams (Team, up to ${users} people). Your payment went through.
Your licence key arrives by email within one business day. Your server keeps working in the meantime.

SayQL, a product of Azenvoc · hello@azenvoc.com`,
      });
      await notifyOwner(
        `Sign a licence: Team for ${licensee}`,
        `<${email}>, subscription ${sub.subscription_id}. Sign and email the key:

sayql-license sign --key ~/.sayql/licence-private.key --licensee "${licensee.replace(/"/g, "'")}" --plan team --users ${users} --expires ${expires}`,
      );
    }
  } catch (e) {
    await release(db, marker).catch(() => {});
    throw e;
  }
}

/** `base64url(payload JSON).base64url(Ed25519 signature)`, as `sayql-license sign` makes it. */
export function signLicence(licence, seedBase64) {
  const seed = Buffer.from(seedBase64, "base64");
  if (seed.length !== 32) throw new Error("LICENCE_SIGNING_KEY must be 32 bytes, base64");
  const key = crypto.createPrivateKey({
    key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), seed]),
    format: "der",
    type: "pkcs8",
  });
  const payload = Buffer.from(JSON.stringify(licence));
  return `${payload.toString("base64url")}.${crypto.sign(null, payload, key).toString("base64url")}`;
}

// ---- helpers ----

async function notifyOwner(subject, text) {
  // A sale goes through even if this note doesn't; it's in Dodo's dashboard too.
  await send({ to: OWNER(), subject, text }).catch((e) => console.error("dodo-webhook: owner note:", e.message));
}

/** Records `id` as handled; false when it already was. */
async function claim(db, id, type) {
  const r = await fetch(`${db.url}/rest/v1/dodo_events`, {
    method: "POST",
    headers: headers(db, { "Content-Type": "application/json", Prefer: "resolution=ignore-duplicates,return=representation" }),
    body: JSON.stringify({ id, type }),
  });
  if (!r.ok) throw new Error(`dodo_events answered ${r.status}`);
  return (await r.json()).length > 0;
}

async function release(db, id) {
  await fetch(`${db.url}/rest/v1/dodo_events?id=eq.${enc(id)}`, { method: "DELETE", headers: headers(db) });
}

async function select(db, path) {
  const r = await fetch(`${db.url}/rest/v1/${path}`, { headers: headers(db) });
  if (!r.ok) throw new Error(`${path.split("?")[0]} answered ${r.status}`);
  return r.json();
}

async function insert(db, table, row) {
  const r = await fetch(`${db.url}/rest/v1/${table}`, {
    method: "POST",
    headers: headers(db, { "Content-Type": "application/json", Prefer: "return=representation" }),
    body: JSON.stringify(row),
  });
  if (!r.ok) throw new Error(`${table} answered ${r.status}`);
  return r.json();
}

const enc = encodeURIComponent;
const sha256 = (s) => crypto.createHash("sha256").update(s).digest("hex");
const newKey = () => "sqlc_" + crypto.randomBytes(24).toString("base64url");
const money = (usd) => `$${usd.toFixed(2)}`;

function addDays(date, days) {
  const d = new Date(`${date}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

function json(status, value) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });
}
