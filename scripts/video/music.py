"""An original background track for the explainer, synthesized from scratch (no samples, no
third-party music), so there's nothing to license.

    python3 scripts/video/music.py OUT.wav DURATION [--intro SECONDS] [--outro SECONDS]

Quiet and clean: a soft sine pad on a slow Cmaj7 – Am7 – Fmaj7 – G6 loop, a few gentle
bell-like notes on top once the title card is over (`--intro`), and just the pad again for
the end card (`--outro` seconds before the end), resolving to C. No drums, nothing sharp.
Needs only numpy.
"""

import argparse
import wave

import numpy as np

SR = 44100
BPM = 72
BEAT = 60 / BPM
CHORD_LEN = 8 * BEAT  # two bars per chord

# MIDI notes: pad voicing, the notes the melody may use, and a soft root.
CHORDS = [
    {"pad": [48, 55, 59, 64], "keys": [76, 79, 83, 72], "root": 36},  # Cmaj7
    {"pad": [45, 52, 55, 60], "keys": [76, 72, 79, 69], "root": 33},  # Am7
    {"pad": [41, 48, 52, 57], "keys": [72, 76, 81, 69], "root": 29},  # Fmaj7
    {"pad": [43, 50, 55, 59], "keys": [74, 71, 79, 76], "root": 31},  # G6
]
FINAL = {"pad": [48, 55, 59, 64], "root": 36}
# When the bell notes play within a chord (in beats) and which of its notes.
KEYS_PATTERN = [(0, 0), (1.5, 1), (3, 2), (5, 3), (6.5, 1)]


def times(n):
    """Sample times for `n` samples. Read-only: numpy 2.2 on Python 3.14 can otherwise reuse a
    named array as scratch space in `t / x` and similar, silently changing it."""
    t = np.arange(n) / SR
    t.flags.writeable = False
    return t


def hz(midi):
    return 440.0 * 2 ** ((midi - 69) / 12)


def swell(n, attack, release):
    """An equal-power rise over `attack` s and fall over the last `release` s, so overlapping
    chords cross-fade without a dip."""
    t = times(n)
    total = n / SR
    rise = np.sin(0.5 * np.pi * np.clip(t / attack, 0, 1))
    fall = np.sin(0.5 * np.pi * np.clip((total - t) / release, 0, 1))
    return rise * fall


def add(buf, start, sig, gain=1.0):
    i = int(start * SR)
    if i >= len(buf):
        return
    sig = sig[: len(buf) - i]
    buf[i : i + len(sig)] += sig * gain


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("duration", type=float)
    ap.add_argument("--intro", type=float, default=3.0)
    ap.add_argument("--outro", type=float, default=4.0)
    a = ap.parse_args()

    total = a.duration
    n = int((total + 4) * SR)
    left = np.zeros(n)
    right = np.zeros(n)
    final_at = total - a.outro

    t0 = 0.0
    i = 0
    while t0 < total:
        last = t0 >= final_at - CHORD_LEN * 0.5
        chord = FINAL if last else CHORDS[i % 4]
        length = (total - t0 + 1) if last else CHORD_LEN + 2.5
        m = int(length * SR)
        t = times(m)
        env = swell(m, 2.5, 2.5 if not last else total - t0)
        # Pad: pure sines with a whisper of the octave, each note panned a little.
        for k, note in enumerate(chord["pad"]):
            f = hz(note)
            tone = np.sin(2 * np.pi * f * t) + 0.12 * np.sin(4 * np.pi * f * t + 1.0)
            pan = (k / (len(chord["pad"]) - 1)) * 0.5 + 0.25
            add(left, t0, tone * env, 0.1 * (1 - pan))
            add(right, t0, tone * env, 0.1 * pan)
        root = np.sin(2 * np.pi * hz(chord["root"]) * t) * env
        add(left, t0, root, 0.05)
        add(right, t0, root, 0.05)
        if last:
            break
        # Bell-like notes: sine with a soft attack and a long, gentle decay, and one quiet echo.
        for beat, idx in KEYS_PATTERN:
            start = t0 + beat * BEAT
            if start < a.intro or start >= final_at - 1.0:
                continue
            mk = int(2.6 * SR)
            tk = times(mk)
            f = hz(chord["keys"][idx])
            bell = (np.sin(2 * np.pi * f * tk) + 0.08 * np.sin(4 * np.pi * f * tk)) * np.exp(-tk * 1.6)
            bell = bell * (0.5 - 0.5 * np.cos(np.pi * np.clip(tk / 0.02, 0, 1)))
            add(left, start, bell, 0.045)
            add(right, start, bell, 0.04)
            add(right, start + BEAT * 0.75, bell, 0.014)
            add(left, start + BEAT * 1.5, bell, 0.009)
        t0 += CHORD_LEN
        i += 1

    mix = np.stack([left, right], axis=1)[: int(total * SR)]
    fade_in = int(1.5 * SR)
    mix[:fade_in] *= (np.linspace(0, 1, fade_in) ** 2)[:, None]
    fade_out = int(3.0 * SR)
    mix[-fade_out:] *= (np.linspace(1, 0, fade_out) ** 2)[:, None]
    mix /= np.max(np.abs(mix)) / 0.7
    pcm = (mix * 32767).astype("<i2")
    with wave.open(a.out, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


if __name__ == "__main__":
    main()
