// Waitlist sign-ups for SayQL for teams (Vercel function).
// Stores the email, the interest and the team size in a Supabase project of their own (never one that
// holds customer data), keyed by email so a second sign-up updates the first. No IP address, user agent
// or cookie is kept. The table, run once in that project's SQL editor:
//
//   create table public.waitlist (
//     email text primary key,
//     interest text not null check (interest in ('team', 'gateway', 'both')),
//     size text,
//     created_at timestamptz not null default now(),
//     updated_at timestamptz not null default now()
//   );
//   alter table public.waitlist enable row level security; -- no policies: only the secret key can use it
//   revoke all on public.waitlist from anon, authenticated;

import { store } from "./_lib/supabase.js";

const INTERESTS = new Set(["team", "gateway", "both"]);
const SIZES = new Set(["1-9", "10-49", "50-249", "250+"]);
const EMAIL = /^[^\s@]{1,64}@[^\s@]{1,255}\.[^\s@]{2,63}$/;


async function upsert({ url, key }, row) {
  const res = await fetch(`${url}/rest/v1/waitlist?on_conflict=email`, {
    method: "POST",
    headers: {
      apikey: key,
      Authorization: `Bearer ${key}`,
      "Content-Type": "application/json",
      Prefer: "resolution=merge-duplicates,return=minimal",
    },
    body: JSON.stringify(row),
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
  const size = SIZES.has(body.size) ? body.size : null;

  const db = store("PUBLIC");
  if (!db) return res.status(503).json({ error: "Sign-ups open in a few days. Please check back soon." });

  try {
    await upsert(db, { email, interest, size, updated_at: new Date().toISOString() });
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
