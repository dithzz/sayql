// Waitlist sign-ups for SayQL for teams (Vercel function).
// Stores the email, the interest and the team size in Upstash Redis (Vercel Marketplace), keyed by
// email so a second sign-up updates the first. No IP address, user agent or cookie is kept.

const INTERESTS = new Set(["team", "gateway", "both"]);
const SIZES = new Set(["1-9", "10-49", "50-249", "250+"]);
const EMAIL = /^[^\s@]{1,64}@[^\s@]{1,255}\.[^\s@]{2,63}$/;

function store() {
  const url = process.env.KV_REST_API_URL || process.env.UPSTASH_REDIS_REST_URL;
  const token = process.env.KV_REST_API_TOKEN || process.env.UPSTASH_REDIS_REST_TOKEN;
  return url && token ? { url, token } : null;
}

async function redis({ url, token }, commands) {
  const res = await fetch(`${url}/multi-exec`, {
    method: "POST",
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: JSON.stringify(commands),
  });
  if (!res.ok) throw new Error(`store answered ${res.status}`);
}

export default async function handler(req, res) {
  if (req.method !== "POST") {
    res.setHeader("Allow", "POST");
    return res.status(405).json({ error: "Use POST." });
  }
  const body = typeof req.body === "string" ? safeJson(req.body) : req.body || {};
  // Bots fill the hidden field; pretend it worked.
  if (body.website) return res.status(200).json({ ok: true });

  const email = String(body.email || "").trim().toLowerCase();
  if (!EMAIL.test(email)) return res.status(400).json({ error: "Please enter a valid email." });
  const interest = INTERESTS.has(body.interest) ? body.interest : "team";
  const size = SIZES.has(body.size) ? body.size : "";

  const kv = store();
  if (!kv) return res.status(503).json({ error: "Sign-ups open in a few days. Please check back soon." });

  const at = new Date().toISOString();
  try {
    await redis(kv, [
      ["HSET", `waitlist:${email}`, "email", email, "interest", interest, "size", size, "at", at],
      ["ZADD", "waitlist", "NX", String(Date.now()), email],
    ]);
  } catch (e) {
    console.error("waitlist: store failed", e.message);
    return res.status(502).json({ error: "That didn't work. Please try again later." });
  }
  return res.status(200).json({ ok: true });
}

function safeJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    return {};
  }
}
