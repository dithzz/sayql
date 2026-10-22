# Explainer video

Records the real app UI (the browser preview with `src/dev/mockBackend.ts`) and turns it
into `media/explainer.mp4`, `media/explainer-light.mp4`, `media/explainer-15s.mp4` and
`media/explainer-poster.png`. Needs Google Chrome, Node 22, Python 3 with numpy, and ffmpeg.

```bash
pnpm exec vite --port 1431 --strictPort          # keep running
node scripts/video/capture.mjs dark              # ~75 s, real time
node scripts/video/capture.mjs light
node scripts/video/render.mjs dark full          # ~2.5 min each
node scripts/video/render.mjs dark short
node scripts/video/render.mjs dark poster
node scripts/video/render.mjs light full
```

- `capture.mjs` holds the storyboard: what to type and click, plus the camera targets
  (element rects) and captions. It opens the preview with `?video`, which seeds the
  `/pii-safe` skill and gives the mock answers for each scene.
- `render.mjs` holds the edit (scene order, the 15-second cut, the site on the end card) and
  draws each frame with `compositor.html`: the window on the brand background, the eased
  zoom, the cursor, the captions and the title and end cards.
- Captures go to `scripts/video/out/` (git-ignored, ~500 MB each).

## Sound

- **Music:** `music.py` synthesizes an original track timed to the edit (no samples or
  licensed music, so nothing to clear). `MUSIC=0` renders silent.
- **Voiceover:** the lines and their cue points are in `voiceover.json`; each render writes
  the script with start times and how long each line can run to `media/<name>-cuts.md`.
  Add a voice either way, then `node scripts/video/render.mjs dark full audio` (only the
  soundtrack is redone) writes `media/explainer-voiceover.mp4`, with the music ducked under
  the voice:
  - recordings: one file per line, `media/voiceover/<id>.wav`;
  - a macOS voice: `VOICE="Ava (Premium)" node scripts/video/render.mjs dark full audio`.
    Premium voices are a free download in System Settings → Accessibility → Spoken Content
    → System voice → Manage Voices; the default ones sound robotic.

When the UI changes, re-run the capture and render. Element lookups go by text and ARIA
labels, so a renamed button needs the same rename in `capture.mjs`.
