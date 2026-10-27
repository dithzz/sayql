// `sayql.dev/install`: adds one to today's count of installer runs, then hands over the script.
// Only the per-day number is kept: no IP address, user agent or anything else about the request.
// If counting fails, the install still goes ahead.

export default async function handler(req, res) {
  const url = process.env.WAITLIST_SUPABASE_URL;
  const key = process.env.WAITLIST_SUPABASE_SECRET_KEY;
  if (url && key && req.method === "GET") {
    try {
      await fetch(`${url.replace(/\/+$/, "")}/rest/v1/rpc/count_install_run`, {
        method: "POST",
        headers: { apikey: key, Authorization: `Bearer ${key}`, "Content-Type": "application/json" },
        body: "{}",
        signal: AbortSignal.timeout(1500),
      });
    } catch (e) {
      console.error("install: count failed", e.message);
    }
  }
  res.setHeader("Cache-Control", "no-store");
  res.redirect(302, "/install.sh");
}
