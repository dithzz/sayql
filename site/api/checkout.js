// POST /api/checkout {"product": "team" | "credits-25" | ...}: starts a Dodo Payments checkout for a
// product the site sells and returns {"url": ...} to send the buyer to (Vercel function). Dodo's page
// takes the email, card and tax details; the key is emailed by /api/dodo-webhook after payment.
// A credit product marked "custom" (pay what you want in Dodo) also takes {"usd": whole dollars}
// between its "min" and "max"; that's the price, and the credit the webhook adds.

import { catalogue, dodo } from "./_lib/dodo.js";

export const config = { runtime: "nodejs" };

const SITE = "https://sayql.dev";

export async function POST(request) {
  if (!process.env.DODO_API_KEY) return json(503, { error: "Buying online isn't available right now. Email hello@azenvoc.com." });
  let body;
  try {
    body = await request.json();
  } catch {
    return json(400, { error: "Send JSON." });
  }
  const name = String(body.product || "");
  const product = catalogue()[name];
  if (!product || !product.id) return json(400, { error: "That isn't something sayql.dev sells." });
  const item = { product_id: product.id, quantity: 1 };
  const metadata = { source: "sayql.dev", product: name };
  if (product.custom) {
    const usd = Number(body.usd);
    const min = Number(product.min) || 5;
    const max = Number(product.max) || 1000;
    if (!Number.isInteger(usd) || usd < min || usd > max) {
      return json(400, { error: `Choose a whole number of dollars from $${min} to $${max}.` });
    }
    item.amount = usd * 100;
    metadata.credit_usd = String(usd);
  }

  try {
    const session = await dodo("/checkouts", {
      method: "POST",
      body: JSON.stringify({
        product_cart: [item],
        return_url: `${SITE}/thanks/?product=${encodeURIComponent(name)}`,
        metadata,
      }),
    });
    if (!session.checkout_url) throw new Error("no checkout_url");
    return json(200, { url: session.checkout_url });
  } catch (e) {
    console.error("checkout:", e.message);
    return json(502, { error: "Couldn't start the checkout. Try again in a minute, or email hello@azenvoc.com." });
  }
}

function json(status, value) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });
}
