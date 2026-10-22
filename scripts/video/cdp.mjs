// A tiny Chrome DevTools Protocol client: launches headless Chrome and talks to one page.
// No dependencies beyond Node 22 (global WebSocket and fetch).
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Starts Chrome and returns a session on its first page. `close()` quits Chrome. */
export async function launch({ width, height, scale = 1, port = 9333 + Math.floor(Math.random() * 500) }) {
  const profile = mkdtempSync(join(tmpdir(), "sayql-video-"));
  const chrome = spawn(
    CHROME,
    [
      "--headless=new",
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${profile}`,
      `--window-size=${width},${height}`,
      `--force-device-scale-factor=${scale}`,
      "--hide-scrollbars",
      "--mute-audio",
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-background-timer-throttling",
      "--disable-renderer-backgrounding",
      "--font-render-hinting=none",
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  let targets;
  for (let i = 0; i < 100 && !targets; i++) {
    await sleep(100);
    targets = await fetch(`http://127.0.0.1:${port}/json`).then((r) => r.json(), () => null);
  }
  const pageTarget = targets.find((t) => t.type === "page");
  const ws = new WebSocket(pageTarget.webSocketDebuggerUrl);
  await new Promise((r, j) => ((ws.onopen = r), (ws.onerror = j)));

  let id = 0;
  const pending = new Map();
  const listeners = new Map();
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(`${msg.error.message} ${msg.error.data ?? ""}`)) : resolve(msg.result);
    } else if (msg.method) {
      for (const fn of listeners.get(msg.method) ?? []) fn(msg.params);
    }
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      pending.set(n, { resolve, reject });
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  const on = (method, fn) => listeners.set(method, [...(listeners.get(method) ?? []), fn]);

  await send("Page.enable");
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: scale, mobile: false });

  /** Evaluates `expr` (a string or a function plus args) in the page and returns its value. */
  const evaluate = async (fn, ...args) => {
    const expression = typeof fn === "function" ? `(${fn})(...${JSON.stringify(args)})` : fn;
    const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  };

  const goto = async (url) => {
    const loaded = new Promise((r) => on("Page.loadEventFired", r));
    await send("Page.navigate", { url });
    await loaded;
  };

  const close = async () => {
    try {
      await send("Browser.close");
    } catch {}
    chrome.kill();
    await sleep(300);
    rmSync(profile, { recursive: true, force: true });
  };

  return { send, on, evaluate, goto, close };
}
