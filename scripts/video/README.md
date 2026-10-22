# Explainer video

Records the real app UI (the browser preview with `src/dev/mockBackend.ts`) and turns it
into `media/explainer.mp4`, `media/explainer-light.mp4`, `media/explainer-15s.mp4` and
`media/explainer-poster.png`. Needs Google Chrome, Node 22 and ffmpeg; nothing to install.

```bash
pnpm exec vite --port 1431 --strictPort          # keep running
node scripts/video/capture.mjs dark              # ~70 s, real time
node scripts/video/capture.mjs light
node scripts/video/render.mjs dark full          # ~2 min each
node scripts/video/render.mjs dark short
node scripts/video/render.mjs dark poster
node scripts/video/render.mjs light full
```

- `capture.mjs` holds the storyboard: what to type and click, plus the camera targets
  (element rects) and captions. It opens the preview with `?video`, which seeds the
  `/pii-safe` skill and gives the mock answers for each scene.
- `render.mjs` holds the edit (scene order, the 15-second cut) and draws each frame with
  `compositor.html`: the window on the brand background, the eased zoom, the cursor, the
  captions and the title and end cards.
- Each render also writes `media/<name>-cuts.md`: where each scene and caption starts, for
  adding music or a voiceover later. The audio track is silent.
- Captures go to `scripts/video/out/` (git-ignored, ~500 MB each).

When the UI changes, re-run the capture and render. Element lookups go by text and ARIA
labels, so a renamed button needs the same rename in `capture.mjs`.
