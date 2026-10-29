// Takes the README's setup screenshot (the "How do you want to use AI?" step) from the UI
// preview (vite + src/dev/mockBackend.ts in ?firstrun mode).
//
//   pnpm exec vite --port 1431 --strictPort        # in another terminal
//   node scripts/screenshots/setup-ai.mjs
//
// Writes .github/assets/feature-setup-ai-{light,dark}.png at 2×: the dialog over the app, on a
// transparent background.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { launch, sleep } from "../video/cdp.mjs";

const URL = process.env.SAYQL_URL ?? "http://localhost:1431/?firstrun";
const VIEW = { width: 900, height: 780, scale: 2 };
const OUT = join(import.meta.dirname, "..", "..", ".github", "assets");

for (const theme of ["light", "dark"]) {
  const page = await launch(VIEW);
  await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: theme }] });
  const rect = (sel, text = null) =>
    page.evaluate(
      (sel, text) => {
        const el = [...document.querySelectorAll(sel)].find((e) => text === null || e.textContent.trim() === text);
        const r = el?.getBoundingClientRect();
        return r && r.width > 0 ? { x: r.x, y: r.y, w: r.width, h: r.height } : null;
      },
      sel,
      text,
    );
  const waitFor = async (sel, text = null) => {
    for (const end = Date.now() + 10000; Date.now() < end; await sleep(50)) {
      const r = await rect(sel, text);
      if (r) return r;
    }
    throw new Error(`Timed out waiting for ${sel} ${text ?? ""}`);
  };
  const click = async (r) => {
    const at = { x: r.x + r.w / 2, y: r.y + r.h / 2 };
    await page.send("Input.dispatchMouseEvent", { type: "mousePressed", ...at, button: "left", clickCount: 1 });
    await page.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...at, button: "left", clickCount: 1 });
    await sleep(300);
  };

  await page.goto(URL);
  await page.evaluate(() => (localStorage.clear(), true));
  await page.goto(URL);
  await page.evaluate(() => document.fonts.ready.then(() => true));
  await sleep(1500);
  await click(await waitFor("button", "Get started"));
  const dialog = await waitFor('[role="dialog"]');
  // Out of the way, so no card shows a hover border.
  await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 1, y: 1 });
  await sleep(400);
  const pad = 28;
  const r = await page.send("Page.captureScreenshot", {
    format: "png",
    clip: { x: dialog.x - pad, y: dialog.y - pad, width: dialog.w + pad * 2, height: dialog.h + pad * 2, scale: 1 },
  });

  const dark = theme === "dark";
  await page.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });
  const width = dialog.w + pad * 2;
  await page.evaluate(
    (html) => {
      document.open();
      document.write(html);
      document.close();
      return true;
    },
    `<style>body{margin:0;background:transparent}.wrap{display:inline-block;padding:32px 36px 44px}
      .card{border-radius:12px;overflow:hidden;border:1px solid ${dark ? "#3a3f4b" : "#d6d9de"};
      box-shadow:0 18px 40px ${dark ? "rgba(23,23,46,.55)" : "rgba(0,42,42,.16)"},0 2px 6px rgba(0,0,0,.08)}img{display:block}</style>
      <div class="wrap" style="width:${width + 2}px"><div class="card"><img src="data:image/png;base64,${r.data}" width="${width}"></div></div>`,
  );
  await page.evaluate(() => Promise.all([...document.images].map((i) => i.decode())).then(() => true));
  const wrap = await rect(".wrap");
  const out = await page.send("Page.captureScreenshot", {
    format: "png",
    clip: { x: wrap.x, y: wrap.y, width: wrap.w, height: wrap.h, scale: 1 },
  });
  writeFileSync(join(OUT, `feature-setup-ai-${theme}.png`), Buffer.from(out.data, "base64"));
  console.log(`wrote feature-setup-ai-${theme}.png`);
  await page.close();
}
