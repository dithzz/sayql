// SayQL AI credits: GET /v1/models, the Anthropic call the SayQL app makes to check a key before
// saving it. Answers with the models credits cover, after checking the key is known, on, and has
// credit left (Vercel function). Spends nothing.

import crypto from "node:crypto";
import { rpc, store } from "../../_lib/supabase.js";

const MODELS = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];

export const config = { runtime: "nodejs" };

export async function GET(request) {
  const db = store("CREDITS");
  if (!db || !process.env.SAYQL_AI_ANTHROPIC_KEY) {
    return error(503, "api_error", "SayQL AI credits aren't available right now.");
  }
  const key = request.headers.get("x-api-key") || "";
  if (!key.startsWith("sqlc_")) return error(401, "authentication_error", "Use a SayQL AI credit key (it starts with sqlc_).");

  let account;
  try {
    [account] = await rpc(db, "ai_account_status", { p_key_sha256: crypto.createHash("sha256").update(key).digest("hex") });
  } catch {
    return error(502, "api_error", "Couldn't check the key. Try again in a moment.");
  }
  if (!account || !account.active) return error(401, "authentication_error", "That SayQL AI credit key isn't valid or was turned off.");
  if (account.balance_micro <= 0) return error(402, "billing_error", "This SayQL AI credit has run out.");

  const data = MODELS.map((id) => ({ type: "model", id, display_name: id }));
  return new Response(JSON.stringify({ data, has_more: false, first_id: MODELS[0], last_id: MODELS.at(-1) }), {
    status: 200,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });
}

function error(status, type, message) {
  return new Response(JSON.stringify({ type: "error", error: { type, message } }), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });
}
