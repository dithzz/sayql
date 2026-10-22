// Records the explainer's scenes from the UI preview (vite + src/dev/mockBackend.ts in ?video mode).
//
//   pnpm exec vite --port 1431 --strictPort        # in another terminal
//   node scripts/video/capture.mjs [dark|light]
//
// Writes scripts/video/out/<theme>/frames/*.jpg (a 30 fps screencast at 2×) and timeline.json:
// scene marks, camera targets (element rects, in CSS px), captions and cursor moves. render.mjs
// turns that into the videos, so the storyboard lives here and render.mjs stays generic.
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { launch, sleep } from "./cdp.mjs";

const theme = process.argv[2] === "light" ? "light" : "dark";
const URL = process.env.SAYQL_URL ?? "http://localhost:1431/?video";
/** The app window in CSS px; frames are twice that. */
export const VIEW = { width: 1440, height: 810, scale: 2 };

const out = join(import.meta.dirname, "out", theme);
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, "frames"), { recursive: true });

const page = await launch(VIEW);
await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: theme }] });
await page.goto(URL);
// A taller editor, so the longest query in the video fits without scrolling.
await page.evaluate(() => localStorage.setItem("split:editor", "440"));
await page.goto(URL);
await page.evaluate(() => document.fonts.ready.then(() => true));
await sleep(2500); // schema load and Monaco
// Headless Chrome only sends screencast frames when something repaints: keep a 1px dot in
// the corner changing (invisible in the video) so frames come steadily.
await page.evaluate(() => {
  const dot = document.createElement("div");
  dot.style.cssText = "position:fixed;right:0;bottom:0;width:1px;height:1px;z-index:99999;pointer-events:none";
  document.body.appendChild(dot);
  let i = 0;
  setInterval(() => (dot.style.background = `rgba(128,128,128,${(i++ % 2) * 0.02 + 0.01})`), 16);
});

// ── Recording ──────────────────────────────────────────────────────────────
const now = () => Date.now() / 1000;
const frames = [];
const events = [];
const log = (type, data = {}) => events.push({ t: now(), type, ...data });
page.on("Page.screencastFrame", (f) => {
  const file = `${String(frames.length).padStart(5, "0")}.jpg`;
  writeFileSync(join(out, "frames", file), Buffer.from(f.data, "base64"));
  frames.push({ t: f.metadata.timestamp, file });
  lastFrame = Date.now();
  void page.send("Page.screencastFrameAck", { sessionId: f.sessionId });
});
const startScreencast = () =>
  page.send("Page.startScreencast", {
    format: "jpeg",
    quality: 88,
    maxWidth: VIEW.width * VIEW.scale,
    maxHeight: VIEW.height * VIEW.scale,
    everyNthFrame: 2,
  });
let lastFrame = Date.now();
let restarts = 0;
await startScreencast();
// The screencast sometimes stops sending frames for good; restart it when that happens.
const watchdog = setInterval(async () => {
  if (Date.now() - lastFrame < 400) return;
  lastFrame = Date.now();
  restarts++;
  await page.send("Page.stopScreencast");
  await startScreencast();
}, 100);

const progress = setInterval(() => process.env.DEBUG && console.log(`${events.at(-1)?.type ?? "-"} frames=${frames.length} restarts=${restarts}`), 2000);

// ── Page helpers ───────────────────────────────────────────────────────────
/** The rect of the element matching `sel` (and whose text is `text`, if given), or null. */
const rect = (sel, text = null) =>
  page.evaluate(
    (sel, text) => {
      const els = [...document.querySelectorAll(sel)].filter((e) => {
        const r = e.getBoundingClientRect();
        return r.width > 0 && r.height > 0 && (text === null || e.textContent.trim() === text || e.getAttribute("aria-label") === text);
      });
      const r = els[0]?.getBoundingClientRect();
      return r ? { x: r.x, y: r.y, w: r.width, h: r.height } : null;
    },
    sel,
    text,
  );

/** Waits until `sel` (with `text`) is on screen and returns its rect. */
async function waitFor(sel, text = null, timeout = 8000) {
  for (const end = Date.now() + timeout; Date.now() < end; await sleep(50)) {
    const r = await rect(sel, text);
    if (r) return r;
  }
  throw new Error(`Timed out waiting for ${sel} ${text ?? ""}`);
}

const waitForText = async (text, timeout = 8000) => {
  for (const end = Date.now() + timeout; Date.now() < end; await sleep(50))
    if (await page.evaluate((t) => document.body.innerText.includes(t), text)) return;
  throw new Error(`Timed out waiting for "${text}"`);
};

const union = (...rs) => {
  const x = Math.min(...rs.map((r) => r.x));
  const y = Math.min(...rs.map((r) => r.y));
  return { x, y, w: Math.max(...rs.map((r) => r.x + r.w)) - x, h: Math.max(...rs.map((r) => r.y + r.h)) - y };
};

let mouse = { x: VIEW.width * 0.62, y: VIEW.height * 0.7 };

/** Glides the (drawn) cursor to the middle of `r`, then clicks there for real. */
async function click(r, { modifiers = 0, glide = 650 } = {}) {
  const to = { x: r.x + r.w / 2, y: r.y + r.h / 2 };
  log("cursor", { x: to.x, y: to.y, dur: glide / 1000 });
  await sleep(glide + 120);
  mouse = to;
  await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...to, modifiers });
  log("click", { x: to.x, y: to.y });
  await page.send("Input.dispatchMouseEvent", { type: "mousePressed", ...to, button: "left", clickCount: 1, modifiers });
  await sleep(70);
  await page.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...to, button: "left", clickCount: 1, modifiers });
  await sleep(250);
}

const KEYS = { Enter: 13, Escape: 27, Backspace: 8 };
async function press(key) {
  // A real Escape stops headless Chrome's screencast for good, so it goes in as a DOM event.
  if (key === "Escape")
    return page.evaluate(() => {
      const el = document.activeElement ?? document.body;
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, cancelable: true }));
      return true;
    });
  const k = { key, code: key, windowsVirtualKeyCode: KEYS[key], nativeVirtualKeyCode: KEYS[key] };
  await page.send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...k });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", ...k });
}

/** Types like a person: uneven gaps, a little longer after spaces. Never long enough to trigger a draft mid-word. */
async function type(text) {
  for (const ch of text) {
    await page.send("Input.insertText", { text: ch });
    await sleep(ch === " " ? 95 + Math.random() * 60 : 55 + Math.random() * 55);
  }
}

const focusAsk = () => page.evaluate(() => document.querySelector('textarea[aria-label="Ask in plain English"]').focus());
const ask = () => waitFor('textarea[aria-label="Ask in plain English"]');
const askBar = async () => {
  const r = await ask();
  return { x: r.x - 40, y: r.y - 14, w: VIEW.width - r.x, h: 70 };
};
const editor = () => waitFor(".monaco-editor");
const results = async () => {
  const head = await waitFor("*", "Results");
  return { x: head.x - 20, y: head.y - 10, w: VIEW.width - head.x + 20, h: VIEW.height - head.y - 20 };
};

const scene = (name) => log("scene", { name });
const cam = (r, opts = {}) => log("cam", { rect: r, ...opts });
const caption = (text) => log("caption", { text });
const hideCursor = () => log("cursor", { hidden: true });

// ── Storyboard ─────────────────────────────────────────────────────────────
// 2. Ask: pick tables, type the question, SQL streams in.
scene("ask");
cam(null);
caption("Pick tables. Ask in plain English.");
await sleep(900);
await click(await waitFor("span, div", "customers"));
await click(await rect("*", "orders"), { modifiers: 4, glide: 380 });
await click(await rect("*", "order_items"), { modifiers: 4, glide: 380 });
await sleep(300);
cam(union(await askBar(), await editor()), { maxZoom: 1.55 });
await sleep(700);
hideCursor();
await focusAsk();
await type("Top 10 customers by spend");
caption("SQL writes itself as you type");
await waitForText("Haiku 5.5 · first SQL");
await sleep(1200);

// 3. Run it.
scene("run");
cam(null);
caption("Run it. Get answers.");
await press("Enter");
await waitForText("Kenji Watanabe");
await sleep(400);
cam(union(await results(), await askBar()), { maxZoom: 1.3 });
await sleep(2600);

// 4. Follow-up.
scene("followup");
cam(union(await askBar(), await editor()), { maxZoom: 1.55 });
caption("Follow up in plain words");
await focusAsk();
await press("Escape");
await sleep(500);
await type("only Europe");
await waitForText("Haiku 5.5 · first SQL");
await sleep(900);
await press("Enter");
await waitForText("Lucas Bernard");
await sleep(300);
cam(union(await results(), await editor()), { maxZoom: 1.25 });
await sleep(2200);

// 5. A skill.
scene("skill");
cam(await askBar(), { maxZoom: 1.9 });
caption("Skills: your rules, every time");
await focusAsk();
await press("Escape");
await sleep(400);
await type("/pii");
await sleep(900);
await press("Enter");
await sleep(400);
caption("/pii-safe: no email, phone or address");
await type("new customers this month");
await sleep(300);
cam(union(await askBar(), await editor()), { maxZoom: 1.55 });
await waitForText("Haiku 5.5 · first SQL");
await sleep(700);
await press("Enter");
await waitForText("Lindqvist");
cam(union(await results(), await editor()), { maxZoom: 1.25 });
await sleep(2400);

// 6. Safety: a change needs your OK.
scene("safety");
cam(union(await askBar(), await editor()), { maxZoom: 1.55 });
caption("Changes always ask first");
await focusAsk();
await press("Escape");
await sleep(400);
await type("Mark order 1042 as shipped");
await waitForText("Haiku 5.5 · first SQL");
await sleep(700);
await press("Enter");
const dialog = await waitFor('[role="dialog"]');
await sleep(300);
cam(dialog, { maxZoom: 1.8, pad: 60 });
await sleep(2300);
await click(await rect('[role="dialog"] button', "Cancel"));
await sleep(500);
cam(null);
await sleep(600);

// 7. A failing query fixes itself.
scene("fix");
cam(union(await askBar(), await editor()), { maxZoom: 1.55 });
caption("Errors fix themselves");
hideCursor();
await focusAsk();
await press("Escape");
await sleep(300);
await type("Newest customers in Japan");
await waitForText("Haiku 5.5 · first SQL");
await sleep(600);
await press("Enter");
await waitForText("Fixing the query with");
await sleep(200);
await waitForText("Haruto");
await sleep(400);
cam(union(await results(), await askBar()), { maxZoom: 1.3 });
await sleep(2200);
log("end");

// Off camera: open the setup guide's AI step.
await click(await rect('button[aria-label="Settings"]'), { glide: 0 });
await click(await waitFor("button", "About"), { glide: 0 });
await click(await waitFor("button", "Show the setup guide again"), { glide: 0 });
await click(await waitFor("button", "Get started"), { glide: 0 });
await waitFor('[role="radiogroup"]');
await sleep(800);

// 8. Any AI.
scene("ai");
cam(union(await waitFor('[role="dialog"]')), { maxZoom: 1.5, pad: 40 });
caption("Use the AI you like");
await sleep(900);
for (const id of ["OpenAI", "Google Gemini", "Ollama"]) {
  const card = await page.evaluate((label) => {
    const el = [...document.querySelectorAll('[role="radio"]')].find((e) => e.textContent.startsWith(label));
    const r = el.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height };
  }, id);
  await click(card, { glide: 500 });
  if (id === "Ollama") caption("Even fully local, with Ollama");
  await sleep(id === "Ollama" ? 2200 : 800);
}
log("end");

// Off camera: close the guide, open Settings → AI apps (MCP).
await press("Escape");
await sleep(400);
if (await rect('[role="dialog"]')) await click(await rect("button", "Set up later"), { glide: 0 }).catch(() => {});
await sleep(300);
if (!(await rect("button", "AI apps (MCP)"))) await click(await rect('button[aria-label="Settings"]'), { glide: 0 });
await click(await waitFor("button", "AI apps (MCP)"), { glide: 0 });
await waitForText("Use SayQL from AI apps (MCP)");
await sleep(800);

// 9. MCP.
scene("mcp");
cam(null);
caption("Claude can query through SayQL");
await sleep(1200);
// The heading through the last setup block.
const content = await page.evaluate(() => {
  const top = [...document.querySelectorAll("main h2")].at(-1).getBoundingClientRect();
  const end = [...document.querySelectorAll("main pre")].at(-1).getBoundingClientRect();
  return { x: Math.min(top.x, end.x), y: top.y, w: Math.max(top.right, end.right) - Math.min(top.x, end.x), h: end.bottom - top.y };
});
cam(content, { maxZoom: 1.6, pad: 30 });
await sleep(1800);
caption("Read-only. Changes are refused.");
await sleep(2400);
log("end");

await sleep(300);
clearInterval(progress);
clearInterval(watchdog);
await page.send("Page.stopScreencast");
await sleep(500);
writeFileSync(join(out, "timeline.json"), JSON.stringify({ theme, view: VIEW, frames, events }, null, 1));
console.log(`${theme}: ${frames.length} frames, ${(frames.at(-1).t - frames[0].t).toFixed(1)} s`);
await page.close();
