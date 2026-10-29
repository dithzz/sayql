// The SayQL AI credits endpoint (site/api/ai) with what Claude Code sends it, against a fake
// Anthropic and a fake credits database (scripts/ai-credits/harness.mjs). No network, no real keys.
import fs from "node:fs";
import { afterAll, beforeAll, beforeEach, expect, it } from "vitest";
import { BROKE_KEY, GOOD_KEY, start } from "./harness.mjs";

let h;
beforeAll(async () => {
  h = await start();
});
afterAll(() => h.close());
beforeEach(() => {
  fs.writeFileSync(h.log, "");
  h.settled.length = 0;
});

const upstream = () =>
  fs
    .readFileSync(h.log, "utf8")
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l))
    .filter((e) => !e.front && !e.front404);

// As Claude Code 2.1.267 sends a turn (headers and body fields seen in a real run).
const CLAUDE_CODE_BETAS =
  "claude-code-20250219,interleaved-thinking-2025-05-14,context-management-2025-06-27,prompt-caching-scope-2026-01-05,mid-conversation-system-2026-04-07,effort-2025-11-24";
function turn(patch = {}, headers = {}) {
  return fetch(`${h.base}/v1/messages?beta=true`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${GOOD_KEY}`,
      "anthropic-version": "2023-06-01",
      "anthropic-beta": CLAUDE_CODE_BETAS,
      "x-claude-code-session-id": "s1",
      ...headers,
    },
    body: JSON.stringify({
      model: "claude-sonnet-5-5",
      max_tokens: 32000,
      stream: true,
      system: [{ type: "text", text: "You are Claude Code.", cache_control: { type: "ephemeral" } }],
      messages: [{ role: "user", content: "Say hi." }],
      tools: [{ name: "Read", description: "Read a file", input_schema: { type: "object", properties: {} } }],
      metadata: { user_id: "u" },
      thinking: { type: "adaptive" },
      context_management: { edits: [{ type: "clear_thinking_20251015", keep: "all" }] },
      output_config: { effort: "high" },
      ...patch,
    }),
  });
}

it("streams Claude Code's turn through with its beta header, SayQL's key upstream, and charges the usage", async () => {
  const r = await turn({}, { "anthropic-beta": `${CLAUDE_CODE_BETAS},oauth-2025-04-20` });
  expect(r.status).toBe(200);
  expect(r.headers.get("content-type")).toContain("text/event-stream");
  const text = await r.text();
  expect(text).toContain("event: ping");
  expect(text).toContain("message_stop");
  const [sent] = upstream();
  expect(sent.path).toBe("/v1/messages");
  expect(sent.headers["anthropic-beta"]).toBe(CLAUDE_CODE_BETAS);
  expect(sent.headers["x-api-key"]).toBe("sk-ant-fake-upstream-key");
  expect(sent.headers.authorization).toBeUndefined();
  expect(sent.body.context_management).toBeTruthy();
  expect(sent.body.tools[0].name).toBe("Read");
  // Usage from the stream: 1200 in, 300 written, 4000 read, 42 out at Sonnet's prices, plus 20%.
  expect(h.settled).toHaveLength(1);
  const s = h.settled[0];
  expect([s.p_model, s.p_input, s.p_cache_write, s.p_cache_read, s.p_output]).toEqual(["claude-sonnet-5-5", 1200, 300, 4000, 42]);
  const cost = (1200 * 2 + 300 * 2 * 1.25 + 4000 * 0.2 + 42 * 10) / 1e6;
  expect(s.p_cost).toBe(Math.ceil(cost * 1e6));
  expect(s.p_charge).toBe(Math.ceil(cost * 1.2 * 1e6));
});

it("holds the 1-hour cache price only when the request asks for it", async () => {
  await (await turn()).text();
  const ttl = { type: "ephemeral", ttl: "1h" };
  await (await turn({ system: [{ type: "text", text: "You are Claude Code.", cache_control: ttl }] })).text();
  const [five, hour] = h.settled.map((s) => s.p_reserved);
  expect(hour).toBeGreaterThan(five);
});

it("passes upstream errors and retry headers through unchanged", async () => {
  const r = await turn({ metadata: { fake_status: 429 } });
  expect(r.status).toBe(429);
  expect(r.headers.get("retry-after")).toBe("7");
  expect(r.headers.get("x-should-retry")).toBe("true");
  expect((await r.json()).error.message).toBe("fake upstream: slow down");
  expect(h.settled[0].p_charge).toBe(0);
});

it("explains a bad key, no credit, a model or tool it doesn't cover", async () => {
  const err = async (r) => [r.status, (await r.json()).error];
  let [status, e] = await err(await turn({}, { authorization: "Bearer sqlc_nobody" }));
  expect([status, e.type]).toEqual([401, "authentication_error"]);
  [status, e] = await err(await turn({}, { authorization: "Bearer sk-ant-not-a-credit-key" }));
  expect(e.message).toContain("starts with sqlc_");
  [status, e] = await err(await turn({}, { authorization: `Bearer ${BROKE_KEY}` }));
  expect([status, e.type, e.message]).toEqual([402, "billing_error", "This SayQL AI credit has run out."]);
  [status, e] = await err(await turn({ model: "claude-sonnet-5" }));
  expect(status).toBe(400);
  expect(e.message).toContain("SayQL AI credits cover these models: claude-opus-5-5, claude-sonnet-5-5, claude-haiku-5-5");
  [status, e] = await err(await turn({ tools: [{ type: "web_search_20260209", name: "web_search" }] }));
  expect(e.message).toContain("not server tools");
  [status, e] = await err(await turn({ speed: "fast" }));
  expect(e.message).toContain("speed");
  expect(upstream()).toHaveLength(0);
});

it("says when the balance is too low for one request's hold", async () => {
  // A huge max_tokens on Opus holds more than the good account's $5.
  const r = await turn({ model: "claude-opus-5-5", max_tokens: 128000, messages: [{ role: "user", content: "x".repeat(900_000) }] });
  expect(r.status).toBe(402);
  expect((await r.json()).error.message).toContain("isn't enough SayQL AI credit left for this request");
});
