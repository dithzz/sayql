import type { TeamServerInfo } from "../../api";

/** "SayQL for teams" on the website. */
export const TEAMS_URL = "https://sayql.dev/#teams";

/** The plan and licence as one line, e.g. "Team plan for Acme · 3 of 10 seats · until 2027-04-30". */
export function licenceText(s: TeamServerInfo): string {
  if (!s.licensed) return "No valid licence: queries can't run until your team admin adds one";
  const plan = s.plan ? s.plan[0].toUpperCase() + s.plan.slice(1) : "Team";
  const parts = [`${plan} plan${s.licensee ? ` for ${s.licensee}` : ""}`];
  if (s.users != null && s.userLimit != null) parts.push(`${s.users} of ${s.userLimit} seats`);
  if (s.expires) parts.push(`until ${s.expires}`);
  return parts.join(" · ");
}

/** The host of a server address, for "sign in to sayql.acme.internal". */
export function serverHost(server: string): string {
  try {
    return new URL(server.includes("://") ? server : `https://${server}`).host || server;
  } catch {
    return server;
  }
}
