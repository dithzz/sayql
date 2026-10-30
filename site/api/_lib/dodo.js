// Dodo Payments, the merchant of record for SayQL purchases on sayql.dev: its API, the product
// catalogue, and webhook signatures. (Vercel doesn't serve files in folders starting with `_`.)
//
// Environment (set in Vercel, never in this repo):
//   DODO_API_KEY         the API key for the mode below
//   DODO_MODE            "test" (default) or "live"
//   DODO_WEBHOOK_SECRET  the endpoint's signing secret (whsec_...)
//   DODO_PRODUCTS        what each product is, by the name the site buys it with, e.g.
//     {"team": {"id": "pdt_...", "kind": "team", "users": 10},
//      "credits-25": {"id": "pdt_...", "kind": "credits", "usd": 25}}

import crypto from "node:crypto";

/** The API's address for the configured mode. */
export function apiBase() {
  return process.env.DODO_MODE === "live" ? "https://live.dodopayments.com" : "https://test.dodopayments.com";
}

/** Products by name, from DODO_PRODUCTS; {} when it's missing or not valid JSON. */
export function catalogue() {
  try {
    const c = JSON.parse(process.env.DODO_PRODUCTS || "{}");
    return c && typeof c === "object" ? c : {};
  } catch {
    return {};
  }
}

/** The catalogue entry for a Dodo product id, with its name; null for products the site doesn't sell. */
export function productById(id) {
  for (const [name, p] of Object.entries(catalogue())) if (p && p.id === id) return { name, ...p };
  return null;
}

/** Calls Dodo's API; throws with its status when it doesn't answer 2xx. */
export async function dodo(path, init = {}) {
  const r = await fetch(`${apiBase()}${path}`, {
    ...init,
    headers: { Authorization: `Bearer ${process.env.DODO_API_KEY}`, "Content-Type": "application/json", ...(init.headers || {}) },
    signal: AbortSignal.timeout(15_000),
  });
  if (!r.ok) throw new Error(`Dodo answered ${r.status} for ${path.split("?")[0]}`);
  return r.json();
}

/**
 * Checks a webhook against its Standard Webhooks signature: HMAC-SHA256 over `{id}.{timestamp}.{body}`
 * with the base64 secret after `whsec_`, any `v1,<sig>` in the header matching, and the timestamp
 * within 5 minutes. `body` is the raw request text.
 */
export function verifyWebhook(body, headers, secret, now = Date.now()) {
  const id = headers.get("webhook-id");
  const ts = headers.get("webhook-timestamp");
  const sigs = headers.get("webhook-signature");
  if (!id || !ts || !sigs || !secret) return false;
  if (!/^\d+$/.test(ts) || Math.abs(now / 1000 - Number(ts)) > 5 * 60) return false;
  const key = Buffer.from(secret.replace(/^whsec_/, ""), "base64");
  const want = crypto.createHmac("sha256", key).update(`${id}.${ts}.${body}`).digest();
  return sigs.split(" ").some((part) => {
    const [version, sig] = part.split(",");
    if (version !== "v1" || !sig) return false;
    const got = Buffer.from(sig, "base64");
    return got.length === want.length && crypto.timingSafeEqual(got, want);
  });
}
