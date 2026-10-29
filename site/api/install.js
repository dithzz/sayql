// `sayql.dev/install`: adds one to today's count of installer runs, then hands over the script.
// Only the per-day number is kept: no IP address, user agent or anything else about the request.
// If counting fails, the install still goes ahead.

import { rpc, store } from "./_lib/supabase.js";

export default async function handler(req, res) {
  const db = store("PUBLIC");
  if (db && req.method === "GET") {
    try {
      await rpc(db, "count_install_run", {}, { signal: AbortSignal.timeout(1500) });
    } catch (e) {
      console.error("install: count failed", e.message);
    }
  }
  res.setHeader("Cache-Control", "no-store");
  res.redirect(302, "/install.sh");
}
