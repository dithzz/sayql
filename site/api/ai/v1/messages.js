// SayQL AI credits: an Anthropic Messages API endpoint that bills a prepaid balance (Vercel function).
//
// A team's SayQL server uses it as its team AI: provider Anthropic, base URL https://sayql.dev/api/ai,
// and a SayQL credit key (sqlc_...) where an Anthropic key would go. Each request:
//   1. holds an upper estimate of its cost from the account's balance (refused if there isn't enough),
//   2. goes to Anthropic with SayQL's own key, the answer streaming straight back,
//   3. is charged what Anthropic reported using, plus the margin, and the hold is returned.
// Prompts and answers pass through and are never stored: the ledger keeps the model and token counts.
//
// Claude Code can use it too (SayQL 0.4.7's audit paid with credits; off since 0.4.8):
// ANTHROPIC_BASE_URL=https://sayql.dev/api/ai and the credit key in ANTHROPIC_AUTH_TOKEN. It posts
// to /v1/messages?beta=true with an `anthropic-beta` header whose values pair with body fields
// (context_management, output_config, ...), so that header is forwarded as sent: Claude Code's
// gateway guide (https://code.claude.com/docs/en/llm-gateway-protocol) says to forward it verbatim
// rather than allowlist values. Costs this endpoint doesn't meter come from body fields and server
// tools, which stay refused below. /v1/messages/count_tokens is optional for Claude Code (it
// estimates instead), so it isn't served.

import crypto from "node:crypto";
import { rpc, store } from "../../_lib/supabase.js";

const UPSTREAM = (process.env.SAYQL_AI_UPSTREAM || "https://api.anthropic.com").replace(/\/+$/, "");
const MARGIN = Number(process.env.SAYQL_AI_MARGIN || "1.2");
const MAX_BODY = 4 * 1024 * 1024;

// USD per million tokens. Cache writes cost 1.25x input (5-minute) or 2x (1-hour).
const PRICES = {
  "claude-opus-5-5": { input: 4, output: 20, cacheRead: 0.2 },
  "claude-sonnet-5-5": { input: 2, output: 10, cacheRead: 0.2 },
  // Haiku 5.5's price rises for prompts over 100K tokens.
  "claude-haiku-5-5": { input: 0.1, output: 0.5, cacheRead: 0.01, long: { over: 100_000, input: 0.5, output: 2.5, cacheRead: 0.05 } },
};
// Request fields that add costs this endpoint doesn't meter.
const REFUSED_FIELDS = ["speed", "mcp_servers", "container", "service_tier", "inference_geo"];
// Request fields dropped before forwarding: `fallbacks` would let another model answer at its own
// price (the SayQL app asks for it on Opus and Sonnet). Without it a rare refusal just ends the reply.
const DROPPED_FIELDS = ["fallbacks"];
// `anthropic-beta` values not forwarded: OAuth belongs to claude.ai sign-ins, and upstream
// requests carry SayQL's API key.
const DROPPED_BETA = /^oauth-/;
// Upstream response headers passed back: Claude Code reads these to decide whether and when to retry.
const PASSED_HEADERS = ["request-id", "retry-after", "x-should-retry"];

export const config = { runtime: "nodejs" };

export async function POST(request) {
  const db = store("CREDITS");
  const anthropicKey = process.env.SAYQL_AI_ANTHROPIC_KEY;
  if (!db || !anthropicKey) return error(503, "api_error", "SayQL AI credits aren't available right now.");

  const key = request.headers.get("x-api-key") || bearer(request.headers.get("authorization"));
  if (!key || !key.startsWith("sqlc_")) {
    return error(401, "authentication_error", "Use a SayQL AI credit key (it starts with sqlc_).");
  }

  const raw = await request.arrayBuffer();
  if (raw.byteLength > MAX_BODY) return error(413, "request_too_large", "The request is too large.");
  let body;
  try {
    body = JSON.parse(Buffer.from(raw).toString("utf8"));
  } catch {
    return error(400, "invalid_request_error", "The request body isn't valid JSON.");
  }
  const price = PRICES[body.model];
  if (!price) {
    return error(400, "invalid_request_error", `SayQL AI credits cover these models: ${Object.keys(PRICES).join(", ")}.`);
  }
  const refused = REFUSED_FIELDS.filter((f) => f in body);
  if (refused.length) return error(400, "invalid_request_error", `SayQL AI credits don't support: ${refused.join(", ")}.`);
  // Only your own tools: server tools (web search and the like) carry charges this endpoint doesn't meter.
  if ((body.tools || []).some((t) => t && t.type && t.type !== "custom")) {
    return error(400, "invalid_request_error", "SayQL AI credits support your own tools only, not server tools.");
  }
  const maxTokens = Number(body.max_tokens);
  if (!(maxTokens > 0 && maxTokens <= 128_000)) return error(400, "invalid_request_error", "Set max_tokens (1 to 128000).");
  for (const f of DROPPED_FIELDS) delete body[f];
  const forward = JSON.stringify(body);

  // Upper estimate: every input byte a token written to the cache (the 1-hour cache when the
  // request asks for it anywhere), and max_tokens of output.
  const bytes = Buffer.byteLength(forward);
  const oneHour = /"ttl"\s*:\s*"1h"/.test(forward);
  const hold = micro(
    cost(price, { input: 0, write5m: oneHour ? 0 : bytes, write1h: oneHour ? bytes : 0, read: 0, output: maxTokens }) * MARGIN,
  );
  const keyHash = crypto.createHash("sha256").update(key).digest("hex");
  let reserved, known;
  try {
    reserved = await rpc(db, "ai_reserve", { p_key_sha256: keyHash, p_amount: hold });
    if (!Array.isArray(reserved) || !reserved.length) known = await rpc(db, "ai_account_status", { p_key_sha256: keyHash });
  } catch (e) {
    // The credits database didn't answer: nothing was reserved, so the request can be retried.
    console.error("ai: reserve failed", e.message);
    return error(503, "api_error", "SayQL AI credits are briefly unavailable. Try again in a moment.");
  }
  if (!Array.isArray(reserved) || !reserved.length) {
    if (!Array.isArray(known) || !known.length || !known[0].active) {
      return error(401, "authentication_error", "That SayQL AI credit key isn't valid or was turned off.");
    }
    if (Number(known[0].balance_micro) > 0) {
      return error(
        402,
        "billing_error",
        "There isn't enough SayQL AI credit left for this request: each request first reserves an upper estimate of its cost. Add credit to continue.",
      );
    }
    return error(402, "billing_error", "This SayQL AI credit has run out.");
  }
  const account = reserved[0].account_id;

  const upstreamHeaders = {
    "content-type": "application/json",
    "x-api-key": anthropicKey,
    "anthropic-version": request.headers.get("anthropic-version") || "2023-06-01",
    accept: request.headers.get("accept") || "application/json",
  };
  const betas = betaValues(request.headers.get("anthropic-beta"));
  if (betas) upstreamHeaders["anthropic-beta"] = betas;
  let upstream;
  try {
    upstream = await fetch(`${UPSTREAM}/v1/messages`, { method: "POST", headers: upstreamHeaders, body: forward });
  } catch (e) {
    await settle(db, account, hold, body.model, null);
    console.error("ai: upstream unreachable", e.message);
    return error(502, "api_error", "Couldn't reach the AI provider. Try again in a moment.");
  }

  const headers = { "content-type": upstream.headers.get("content-type") || "application/json", "cache-control": "no-store" };
  for (const h of PASSED_HEADERS) {
    const v = upstream.headers.get(h);
    if (v) headers[h] = v;
  }

  if (!upstream.ok || !upstream.body) {
    // Anthropic doesn't charge failed requests: hand back the hold and pass the error on.
    const text = await upstream.text();
    await settle(db, account, hold, body.model, null);
    return new Response(text, { status: upstream.status, headers });
  }

  if (!String(headers["content-type"]).includes("text/event-stream")) {
    const text = await upstream.text();
    let usage = null;
    try {
      usage = JSON.parse(text).usage || null;
    } catch {}
    await settle(db, account, hold, body.model, usage);
    return new Response(text, { status: upstream.status, headers });
  }

  // Streaming: pass every byte through and read the usage on the way.
  const meter = new SseUsage();
  const reader = upstream.body.getReader();
  let done = false;
  const out = new ReadableStream({
    async pull(controller) {
      try {
        const { value, done: end } = await reader.read();
        if (end) {
          // Settle before closing: once the stream ends the function may be stopped.
          if (!done) {
            done = true;
            await settle(db, account, hold, body.model, meter.usage());
          }
          controller.close();
          return;
        }
        meter.push(value);
        controller.enqueue(value);
      } catch (e) {
        controller.error(e);
        if (!done) {
          done = true;
          await settle(db, account, hold, body.model, meter.usage(), true);
        }
      }
    },
    async cancel() {
      // The team's server went away mid-answer: stop generating, charge what was used so far.
      await reader.cancel().catch(() => {});
      if (!done) {
        done = true;
        await settle(db, account, hold, body.model, meter.usage(), true);
      }
    },
  });
  return new Response(out, { status: upstream.status, headers });
}

/** Reads Anthropic's server-sent events for usage: input and cache counts in message_start,
 *  the output count in message_delta (sent at the end). Also counts the generated text, so a
 *  stream that breaks before message_delta is still charged for its output. */
export class SseUsage {
  constructor() {
    this.buffer = "";
    this.u = null;
    this.final = false;
    this.streamedChars = 0;
    this.decoder = new TextDecoder();
  }
  push(bytes) {
    this.buffer += this.decoder.decode(bytes, { stream: true });
    let cut;
    while ((cut = this.buffer.indexOf("\n")) >= 0) {
      const line = this.buffer.slice(0, cut).trim();
      this.buffer = this.buffer.slice(cut + 1);
      if (!line.startsWith("data:")) continue;
      let ev;
      try {
        ev = JSON.parse(line.slice(5));
      } catch {
        continue;
      }
      if (ev.type === "message_start" && ev.message && ev.message.usage) this.u = { ...ev.message.usage };
      else if (ev.type === "message_delta" && ev.usage) {
        this.u = { ...(this.u || {}), ...ev.usage };
        this.final = true;
      } else if (ev.type === "content_block_delta" && ev.delta) {
        const d = ev.delta;
        this.streamedChars += String(d.text || d.thinking || d.partial_json || "").length;
      }
    }
  }
  /** The usage to charge. Before message_delta the output count is a placeholder, so it is at
   *  least the streamed text at about 3 characters a token (on the high side for English). */
  usage() {
    if (!this.u || this.final) return this.u;
    const streamed = Math.ceil(this.streamedChars / 3);
    return { ...this.u, output_tokens: Math.max(this.u.output_tokens || 0, streamed) };
  }
}

/** Charges what the usage says, or (with no usage on a broken stream) the whole hold. */
async function settle(db, account, hold, model, usage, broken = false) {
  let costUsd = 0;
  let tokens = { input: 0, write5m: 0, write1h: 0, read: 0, output: 0 };
  if (usage) {
    const write = usage.cache_creation_input_tokens || 0;
    const split = usage.cache_creation || {};
    const w1h = split.ephemeral_1h_input_tokens || 0;
    tokens = {
      input: usage.input_tokens || 0,
      write5m: split.ephemeral_5m_input_tokens != null ? split.ephemeral_5m_input_tokens : write - w1h,
      write1h: w1h,
      read: usage.cache_read_input_tokens || 0,
      output: usage.output_tokens || 0,
    };
    costUsd = cost(PRICES[model], tokens);
  }
  // The real charge, even if it is a little over the hold (the balance can then dip below zero once,
  // and the next request is refused); with no usage at all, a broken stream keeps the hold.
  const charge = usage ? micro(costUsd * MARGIN) : broken ? hold : 0;
  try {
    await rpc(db, "ai_settle", {
      p_account: account,
      p_reserved: hold,
      p_charge: charge,
      p_cost: micro(costUsd),
      p_model: model,
      p_input: tokens.input,
      p_cache_write: tokens.write5m + tokens.write1h,
      p_cache_read: tokens.read,
      p_output: tokens.output,
    });
  } catch (e) {
    // The hold stays taken, so the balance errs on the side of the account being charged too much,
    // never too little. It shows in the owner portal as a hold without a usage row.
    console.error("ai: settle failed", account, e.message);
  }
}

/** USD for these tokens at the model's prices (long-prompt prices past the model's threshold). */
function cost(p, t) {
  const prompt = t.input + t.write5m + t.write1h + t.read;
  const r = p.long && prompt > p.long.over ? p.long : p;
  return (
    (t.input * r.input + t.write5m * r.input * 1.25 + t.write1h * r.input * 2 + t.read * r.cacheRead + t.output * r.output) /
    1_000_000
  );
}

function micro(usd) {
  return Math.ceil(usd * 1_000_000);
}

/** The client's `anthropic-beta` values to forward, or null for none. */
export function betaValues(h) {
  const kept = String(h || "")
    .split(",")
    .map((v) => v.trim())
    .filter((v) => v && !DROPPED_BETA.test(v));
  return kept.length ? kept.join(",") : null;
}

function bearer(h) {
  return h && h.toLowerCase().startsWith("bearer ") ? h.slice(7).trim() : null;
}

function error(status, type, message) {
  return new Response(JSON.stringify({ type: "error", error: { type, message } }), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });
}


