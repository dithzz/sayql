// SayQL AI credits: GET /v1/balance, what's left on a credit key, so the SayQL app and team server
// can show it and warn before it runs out (Vercel function). Spends nothing and changes nothing.
// Not part of Anthropic's API: only SayQL's own clients call it.

import crypto from "node:crypto";
import { rpc, store } from "../../_lib/supabase.js";

export const config = { runtime: "nodejs" };

export async function GET(request) {
  const db = store("CREDITS");
  if (!db) return error(503, "api_error", "SayQL AI credits aren't available right now.");
  const key = request.headers.get("x-api-key") || bearer(request.headers.get("authorization"));
  if (!key || !key.startsWith("sqlc_")) return error(401, "authentication_error", "Use a SayQL AI credit key (it starts with sqlc_).");

  let account;
  try {
    [account] = await rpc(db, "ai_account_status", { p_key_sha256: crypto.createHash("sha256").update(key).digest("hex") });
  } catch {
    return error(502, "api_error", "Couldn't check the balance. Try again in a moment.");
  }
  if (!account || !account.active) return error(401, "authentication_error", "That SayQL AI credit key isn't valid or was turned off.");

  // Whole cents, rounded down: never show more than is there.
  const usd = Math.max(0, Math.floor(Number(account.balance_micro) / 10_000) / 100);
  return new Response(JSON.stringify({ balance_usd: usd }), {
    status: 200,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });
}

function bearer(header) {
  const m = /^Bearer\s+(\S+)$/i.exec(header || "");
  return m ? m[1] : "";
}

function error(status, type, message) {
  return new Response(JSON.stringify({ type: "error", error: { type, message } }), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });
}
