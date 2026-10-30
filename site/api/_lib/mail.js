// Email from sayql.dev, through Resend: keys after a purchase, and a note to the owner per sale.
//
// Environment (set in Vercel, never in this repo):
//   RESEND_API_KEY  Resend's API key, for a verified sending domain
//   MAIL_FROM       e.g. "SayQL <keys@sayql.dev>" (default)
//   MAIL_REPLY_TO   where replies go (default hello@azenvoc.com)
//   OWNER_EMAIL     who hears about each sale (default hello@azenvoc.com)
//   RESEND_API_URL  only for tests

export const OWNER = () => process.env.OWNER_EMAIL || "hello@azenvoc.com";

/** Sends a plain-text email; throws when the mail service doesn't take it. */
export async function send({ to, subject, text }) {
  if (!process.env.RESEND_API_KEY) throw new Error("Email isn't set up (RESEND_API_KEY).");
  const r = await fetch(process.env.RESEND_API_URL || "https://api.resend.com/emails", {
    method: "POST",
    headers: { Authorization: `Bearer ${process.env.RESEND_API_KEY}`, "Content-Type": "application/json" },
    body: JSON.stringify({
      from: process.env.MAIL_FROM || "SayQL <keys@sayql.dev>",
      reply_to: process.env.MAIL_REPLY_TO || "hello@azenvoc.com",
      to: [to],
      subject,
      text,
    }),
    signal: AbortSignal.timeout(15_000),
  });
  if (!r.ok) throw new Error(`The mail service answered ${r.status}`);
}
