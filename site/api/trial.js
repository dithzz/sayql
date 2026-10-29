// Opt-in trial sign-ups (Vercel function). A SayQL server's admin page shows an optional form;
// only when its admin clicks Send does their browser post what they typed here: company, email,
// team size and the server's version. Nothing is sent otherwise, and no IP address, user agent or
// cookie is kept. Stored in the Supabase project that holds the waitlist (table `trials`).

import { store } from "./_lib/supabase.js";

const SIZES = new Set(["1-9", "10-49", "50-249", "250+"]);
const EMAIL = /^[^\s@]{1,64}@[^\s@]{1,255}\.[^\s@]{2,63}$/;
const VERSION = /^\d{1,3}\.\d{1,3}\.\d{1,3}$/;


export default async function handler(req, res) {
  // Sent from each customer's own server address, so any origin may post; no cookies are involved.
  res.setHeader("Access-Control-Allow-Origin", "*");
  res.setHeader("Access-Control-Allow-Methods", "POST, OPTIONS");
  res.setHeader("Access-Control-Allow-Headers", "Content-Type");
  if (req.method === "OPTIONS") return res.status(204).end();
  if (req.method !== "POST") {
    res.setHeader("Allow", "POST, OPTIONS");
    return res.status(405).json({ error: "Use POST." });
  }
  const body = typeof req.body === "string" ? safeJson(req.body) : req.body || {};
  const company = String(body.company || "").trim().slice(0, 200);
  const email = String(body.email || "").trim().toLowerCase();
  if (!company) return res.status(400).json({ error: "Please enter your company." });
  if (!EMAIL.test(email)) return res.status(400).json({ error: "Please enter a valid email." });
  const size = SIZES.has(body.size) ? body.size : null;
  const version = VERSION.test(String(body.version || "")) ? body.version : null;

  const db = store("PUBLIC");
  if (!db) return res.status(503).json({ error: "That didn't work. Please try again later." });
  try {
    const r = await fetch(`${db.url}/rest/v1/trials`, {
      method: "POST",
      headers: {
        apikey: db.key,
        Authorization: `Bearer ${db.key}`,
        "Content-Type": "application/json",
        Prefer: "return=minimal",
      },
      body: JSON.stringify({ company, email, size, server_version: version }),
    });
    if (!r.ok) throw new Error(`store answered ${r.status}`);
  } catch (e) {
    console.error("trial: store failed", e.message);
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
