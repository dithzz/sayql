// The Supabase project behind sayql.dev's functions, shared so the functions don't each carry a
// copy. (Vercel doesn't serve files in folders starting with `_` as routes.)
//
// Each function names its scope, and uses that scope's own key when one is set:
//   SUPABASE_KEY_PUBLIC   waitlist, trial, install counter (public, unauthenticated endpoints)
//   SUPABASE_KEY_CREDITS  SayQL AI credits (reserve, settle, account status)
//   SUPABASE_KEY_OWNER    the owner portal
// Until they are set, every scope falls back to WAITLIST_SUPABASE_SECRET_KEY. Give each its own
// restricted key, so a bug in a public endpoint can't reach the credits tables.

/** The project's address and the key for `scope`, or null when not configured. */
export function store(scope) {
  const url = process.env.WAITLIST_SUPABASE_URL;
  const key = process.env[`SUPABASE_KEY_${scope}`] || process.env.WAITLIST_SUPABASE_SECRET_KEY;
  return url && key ? { url: url.replace(/\/+$/, ""), key } : null;
}

export function headers(db, extra = {}) {
  return { apikey: db.key, Authorization: `Bearer ${db.key}`, ...extra };
}

/** Calls a database function; throws when it doesn't answer 2xx. */
export async function rpc(db, fn, args = {}, { signal } = {}) {
  const r = await fetch(`${db.url}/rest/v1/rpc/${fn}`, {
    method: "POST",
    headers: headers(db, { "Content-Type": "application/json" }),
    body: JSON.stringify(args),
    signal,
  });
  if (!r.ok) throw new Error(`${fn} answered ${r.status}`);
  return r.json();
}
