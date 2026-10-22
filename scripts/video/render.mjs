// Turns a capture (capture.mjs) into the videos: the app as a window on a brand background,
// eased zoom and pan onto each camera target, a drawn cursor, captions and title/end cards.
//
//   node scripts/video/render.mjs [dark|light] [full|short|poster]
//
// Frames are drawn by compositor.html in headless Chrome and piped to ffmpeg (H.264, 30 fps,
// with a silent AAC track to drop music or a voiceover onto). Cut points go to media/*-cuts.md.
import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { launch } from "./cdp.mjs";

const theme = process.argv[2] === "light" ? "light" : "dark";
const cut = process.argv[3] ?? "full";
const FFMPEG = process.env.FFMPEG ?? "/opt/homebrew/bin/ffmpeg";
const FPS = 30;
const XFADE = 0.45;
/** Where the app window sits in the 1920×1080 frame at zoom 1. */
const WIN = { x: 160, y: 46, w: 1600, h: 900 };

const root = join(import.meta.dirname, "..", "..");
const capture = join(import.meta.dirname, "out", theme);
const { view, frames, events } = JSON.parse(readFileSync(join(capture, "timeline.json"), "utf8"));
const S = WIN.w / view.width; // CSS px → stage px

// ── The edit ───────────────────────────────────────────────────────────────
// Clips name capture scenes; `until` ends a clip at a later scene's start (or its "end" mark),
// `trim` drops seconds from its start, `max` caps its length, `speed` > 1 plays it faster.
const EDITS = {
  full: [
    { card: "title", dur: 3.2 },
    { scene: "ask", until: "followup" },
    { scene: "followup", until: "skill" },
    { scene: "skill", until: "safety" },
    { scene: "safety", until: "fix" },
    { scene: "fix" },
    { scene: "ai" },
    { scene: "mcp" },
    { card: "end", dur: 4 },
  ],
  short: [
    { card: "title", dur: 1.4 },
    { scene: "ask", until: "followup", speed: 1.35, trim: 1.5 },
    { scene: "followup", until: "skill", speed: 1.5, max: 3.4 },
    { card: "end", dur: 2.2 },
  ],
};
EDITS.poster = EDITS.full;

const sceneStart = (name) => events.find((e) => e.type === "scene" && e.name === name).t;
/** The first scene or "end" mark after `t`. */
const nextMark = (t) => events.find((e) => (e.type === "scene" || e.type === "end") && e.t > t + 1e-6).t;

const items = EDITS[cut].map((it) => {
  if (it.card) return { ...it, kind: "card" };
  const speed = it.speed ?? 1;
  const from = sceneStart(it.scene) + (it.trim ?? 0);
  let to = (it.until ? sceneStart(it.until) : nextMark(sceneStart(it.scene))) - (it.trimEnd ?? 0);
  if (it.max) to = Math.min(to, from + it.max * speed);
  return { ...it, kind: "clip", from, to, speed, dur: (to - from) / speed };
});
// Clips that continue where the previous one stopped play straight on; others cross-fade.
let t = 0;
items.forEach((it, i) => {
  const prev = items[i - 1];
  it.contiguous = prev?.kind === "clip" && it.kind === "clip" && Math.abs(prev.to - it.from) < 0.05;
  it.start = i === 0 || it.contiguous ? t : t - XFADE;
  t = it.start + it.dur;
});
const total = t;

// ── State at a source time ─────────────────────────────────────────────────
const ease = (x) => (x <= 0 ? 0 : x >= 1 ? 1 : x < 0.5 ? 4 * x * x * x : 1 - (-2 * x + 2) ** 3 / 2);
const FULL = { z: 1, cx: 960, cy: 540 };

/** Zoom and centre that frame `rect` (CSS px) above the caption, clamped inside the frame. */
function framing(e) {
  if (!e.rect) return FULL;
  const pad = (e.pad ?? 24) * S;
  const r = { x: WIN.x + e.rect.x * S - pad, y: WIN.y + e.rect.y * S - pad, w: e.rect.w * S + 2 * pad, h: e.rect.h * S + 2 * pad };
  const z = Math.max(1, Math.min(e.maxZoom ?? 2, (1920 * 0.96) / r.w, 900 / r.h));
  // Keep the view inside the window where it's big enough, else centred on it.
  const clamp = (v, half, lo, size) => (size <= 2 * half ? lo + size / 2 : Math.min(Math.max(v, lo + half), lo + size - half));
  // Centre the target in the 0–930 px band above the caption.
  const cx = clamp(r.x + r.w / 2, 960 / z, WIN.x, WIN.w);
  const cy = clamp(r.y + r.h / 2 + (540 - 465) / z, 540 / z, WIN.y, WIN.h);
  return { z, cx, cy };
}

/** Camera at source time `ts`; the clip's first camera move snaps if the clip was cut into. */
function cameraAt(ts, clip) {
  const CAM_S = 0.95;
  let move = { from: FULL, to: FULL, t0: -Infinity };
  const at = (m, t) => {
    const k = ease((t - m.t0) / CAM_S);
    const lz = Math.log(m.from.z) + (Math.log(m.to.z) - Math.log(m.from.z)) * k;
    return { z: Math.exp(lz), cx: m.from.cx + (m.to.cx - m.from.cx) * k, cy: m.from.cy + (m.to.cy - m.from.cy) * k };
  };
  for (const e of events) {
    if (e.t > ts) break;
    if (e.type !== "cam") continue;
    const snap = !clip.contiguous && e.t <= clip.from + 0.15;
    move = snap ? { from: framing(e), to: framing(e), t0: e.t } : { from: at(move, e.t), to: framing(e), t0: e.t };
  }
  return at(move, ts);
}

function cursorAt(ts) {
  let pos = { x: view.width * 0.62, y: view.height * 0.7 };
  let glide = null;
  let visible = false;
  let ripple = null;
  for (const e of events) {
    if (e.t > ts) break;
    if (e.type === "cursor" && e.hidden) visible = false;
    else if (e.type === "cursor") {
      const from = glide ? glideAt(glide, e.t) : pos;
      glide = { from, to: { x: e.x, y: e.y }, t0: e.t, dur: e.dur || 0.001 };
      pos = glide.to;
      visible = true;
    } else if (e.type === "click" && ts - e.t < 0.4) ripple = { x: e.x, y: e.y, p: (ts - e.t) / 0.4 };
  }
  const p = glide ? glideAt(glide, ts) : pos;
  const st = (v) => ({ x: WIN.x + v.x * S, y: WIN.y + v.y * S });
  return { cursor: { ...st(p), visible }, ripple: ripple && { ...st(ripple), p: ripple.p } };
}
function glideAt(g, t) {
  const k = ease((t - g.t0) / g.dur);
  return { x: g.from.x + (g.to.x - g.from.x) * k, y: g.from.y + (g.to.y - g.from.y) * k };
}

/** The caption showing at source time `ts` within `clip`, with fades at its edges. */
function captionAt(ts, clip) {
  const caps = events.filter((e) => e.type === "caption" && e.t >= clip.from - 0.2 && e.t < clip.to);
  const i = caps.findLastIndex((e) => e.t <= ts);
  // A clip trimmed past its scene's first caption still shows it.
  const cur = i >= 0 ? caps[i] : events.findLast((e) => e.type === "caption" && e.t <= ts);
  if (!cur?.text) return null;
  const until = i + 1 < caps.length ? caps[i + 1].t : null;
  const fadeIn = Math.min(1, (ts - Math.max(cur.t, clip.from)) / clip.speed / 0.3);
  const fadeOut = until === null ? 1 : Math.min(1, (until - ts) / clip.speed / 0.2);
  return { text: cur.text, opacity: Math.max(0, Math.min(fadeIn, fadeOut)) };
}

const frameAt = (ts) => {
  let lo = 0;
  let hi = frames.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (frames[mid].t <= ts) lo = mid;
    else hi = mid - 1;
  }
  return pathToFileURL(join(capture, "frames", frames[lo].file)).href;
};

/** Everything compositor.html needs for output time `t`. */
function stateAt(t) {
  const s = { layers: [], card: null, caption: null };
  for (const it of items) {
    if (t < it.start || t >= it.start + it.dur) continue;
    const local = t - it.start;
    const next = items[items.indexOf(it) + 1];
    const fadesOut = next && !next.contiguous;
    let opacity = 1;
    if (!it.contiguous && it.start > 0) opacity = Math.min(opacity, local / XFADE);
    if (fadesOut) opacity = Math.min(opacity, (it.dur - local) / XFADE);
    opacity = ease(Math.max(0, Math.min(1, opacity)));
    if (it.kind === "card") {
      s.card = { kind: it.card, opacity, rise: (1 - ease(Math.min(1, local / 0.8))) * 16 };
      continue;
    }
    const ts = it.from + local * it.speed;
    const cap = captionAt(ts, it);
    if (cap) s.caption = { ...cap, opacity: cap.opacity * opacity };
    s.layers.push({ src: frameAt(ts), opacity, cam: cameraAt(ts, it), ...cursorAt(ts) });
  }
  // The incoming layer draws on top during a cross-fade, so the outgoing one stays opaque.
  if (s.layers.length === 2) s.layers[0].opacity = 1;
  return s;
}

// ── Render ─────────────────────────────────────────────────────────────────
const page = await launch({ width: 1920, height: 1080, scale: 1 });
await page.goto(pathToFileURL(join(import.meta.dirname, "compositor.html")).href);
const wordmark = pathToFileURL(join(root, ".github", "assets", `wordmark-${theme}.svg`)).href;
const interLoaded = await page.evaluate(`setup(${JSON.stringify({ theme, win: WIN, wordmark })})`);
if (!interLoaded) console.warn("Inter didn't load (offline?); captions fall back to the system font.");

const shot = async (t, format = "jpeg") => {
  await page.evaluate(`frame(${JSON.stringify(stateAt(t))})`);
  const r = await page.send("Page.captureScreenshot", { format, quality: format === "jpeg" ? 95 : undefined });
  return Buffer.from(r.data, "base64");
};

const suffix = theme === "light" ? "-light" : "";
if (cut === "poster") {
  // The first question's SQL, streamed in, with its caption: just before it runs.
  const ask = items.find((it) => it.scene === "ask");
  const t = ask.start + (sceneStart("run") - 0.15 - ask.from) / ask.speed;
  writeFileSync(join(root, "media", `explainer${suffix}-poster.png`), await shot(t, "png"));
} else {
  const name = cut === "short" ? `explainer${suffix}-15s` : `explainer${suffix}`;
  const file = join(root, "media", `${name}.mp4`);
  const ff = spawn(
    FFMPEG,
    [
      "-y", "-loglevel", "error",
      "-f", "image2pipe", "-framerate", String(FPS), "-c:v", "mjpeg", "-i", "-",
      "-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo",
      "-map", "0:v", "-map", "1:a", "-shortest",
      "-c:v", "libx264", "-preset", "slow", "-crf", process.env.CRF ?? "22", "-tune", "animation",
      "-pix_fmt", "yuv420p", "-profile:v", "high", "-r", String(FPS), "-g", String(FPS * 2),
      "-c:a", "aac", "-b:a", "32k",
      "-movflags", "+faststart",
      file,
    ],
    { stdio: ["pipe", "inherit", "inherit"] },
  );
  const n = Math.round(total * FPS);
  for (let i = 0; i < n; i++) {
    const buf = await shot(i / FPS);
    if (!ff.stdin.write(buf)) await new Promise((r) => ff.stdin.once("drain", r));
    if (i % 150 === 0) process.stdout.write(`\r${name}: ${((i / n) * 100).toFixed(0)}%`);
  }
  ff.stdin.end();
  await new Promise((r) => ff.on("close", r));
  console.log(`\r${name}: ${total.toFixed(1)} s → ${file}`);

  // Where each part starts, for laying music or a voiceover on later.
  const fmt = (s) => `${Math.floor(s / 60)}:${(s % 60).toFixed(1).padStart(4, "0")}`;
  const rows = items.map((it) => {
    const caps = it.kind === "clip" ? events.filter((e) => e.type === "caption" && e.text && e.t >= it.from - 0.2 && e.t < it.to) : [];
    const lines = caps.map((c) => `    - ${fmt(it.start + Math.max(0, c.t - it.from) / it.speed)} caption: “${c.text}”`);
    return [`- ${fmt(it.start)} ${it.card ? `${it.card} card` : it.scene}${it.contiguous ? " (continues, no cut)" : " (cut)"}`, ...lines].join("\n");
  });
  writeFileSync(
    join(root, "media", `${name}-cuts.md`),
    `# ${name}.mp4 — cut points\n\n${total.toFixed(1)} s, ${FPS} fps, 1920×1080. Cross-fades are ${XFADE} s and start at the time shown.\nThe audio track is silent.\n\n${rows.join("\n")}\n`,
  );
}
await page.close();
