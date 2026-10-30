// `sayql.dev/api/setup-check`: the team server installer asks how its machine looks from the
// internet. It answers with the caller's public IPv4 address and, given `?domain=`, where that name
// points. With `&probe=1`, and only when the name points at the caller itself, it also says whether
// ports 80 and 443 answer there, which is what Let's Encrypt needs for the certificate.
// Plain `key=value` lines, easy to read in a shell script. Nothing is stored or logged.

import dns from "node:dns/promises";
import http from "node:http";
import https from "node:https";

const NAME = /^(?=.{1,253}$)([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$/;
const IPV4 = /^\d{1,3}(\.\d{1,3}){3}$/;

export default async function handler(req, res) {
  res.setHeader("Cache-Control", "no-store");
  res.setHeader("Content-Type", "text/plain; charset=utf-8");
  if (req.method !== "GET") {
    res.setHeader("Allow", "GET");
    return res.status(405).send("error=Use GET.\n");
  }
  const ip = callerIp(req);
  const out = [`ip=${ip}`];
  const domain = String(req.query?.domain || "").trim().toLowerCase().replace(/\.$/, "");
  if (domain) {
    if (!NAME.test(domain)) return res.status(400).send(`${out.join("\n")}\nerror=That isn't a domain name.\n`);
    const addresses = await resolve(domain);
    const here = Boolean(ip) && addresses.includes(ip);
    out.push(`domain=${domain}`, `addresses=${addresses.join(" ")}`, `points_here=${here ? "yes" : "no"}`);
    // Only the caller's own machine is ever probed, and at the address checked above, so a name
    // can't be switched to point somewhere else between the check and the probe.
    if (here && req.query?.probe) {
      const [web, secure] = await Promise.all([answers(http, domain, ip, 80), answers(https, domain, ip, 443)]);
      out.push(`http=${web ? "yes" : "no"}`, `https=${secure ? "yes" : "no"}`);
    }
  }
  return res.status(200).send(out.join("\n") + "\n");
}

function callerIp(req) {
  const h = req.headers || {};
  const raw = String(h["x-real-ip"] || h["x-forwarded-for"] || "").split(",")[0].trim();
  return IPV4.test(raw) ? raw : "";
}

async function resolve(domain) {
  try {
    return await dns.resolve4(domain);
  } catch {
    return [];
  }
}

// True when something at ip:port answers HTTP(S) for this name with a valid certificate.
function answers(mod, domain, ip, port) {
  return new Promise((done) => {
    const req = mod.request(
      {
        host: domain,
        port,
        path: "/",
        method: "HEAD",
        timeout: 4000,
        lookup: (_host, opts, cb) => (opts?.all ? cb(null, [{ address: ip, family: 4 }]) : cb(null, ip, 4)),
      },
      (r) => {
        r.resume();
        done(true);
      },
    );
    req.on("timeout", () => req.destroy(new Error("timeout")));
    req.on("error", () => done(false));
    req.end();
  });
}
