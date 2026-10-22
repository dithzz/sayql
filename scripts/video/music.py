"""An original background track for the explainer, synthesized from scratch (no samples, no
third-party music), so there's nothing to license.

    python3 scripts/video/music.py OUT.wav DURATION [--intro SECONDS] [--outro SECONDS]

A calm 100 BPM loop (C – G – Am – F): a soft pad and a plucked arpeggio from the start, a
sub bass, kick and hats once the title card is over (`--intro`), back to just the pad and
pluck for the end card (`--outro` seconds before the end), ending on a held C chord.
Needs only numpy.
"""

import argparse
import wave

import numpy as np

SR = 44100
BPM = 100
BEAT = 60 / BPM
BAR = 4 * BEAT

# MIDI notes. Pad voicings (open, with added 9ths) and the bass root for each chord.
CHORDS = [
    {"pad": [48, 55, 62, 64], "arp": [72, 76, 79, 74], "bass": 36},  # Cadd9
    {"pad": [43, 50, 57, 59], "arp": [71, 74, 79, 69], "bass": 31},  # G(add9)
    {"pad": [45, 52, 59, 60], "arp": [72, 76, 81, 71], "bass": 33},  # Am(add9)
    {"pad": [41, 48, 55, 57], "arp": [69, 72, 77, 67], "bass": 29},  # Fmaj9-ish
]
ARP_PATTERN = [0, 1, 2, 3, 2, 1, 3, 1]  # eighth notes over one bar


def times(n):
    """Sample times for `n` samples. Read-only: numpy 2.2 on Python 3.14 can otherwise reuse a
    named array as scratch space in `t / x` and similar, silently changing it."""
    t = np.arange(n) / SR
    t.flags.writeable = False
    return t


def hz(midi):
    return 440.0 * 2 ** ((midi - 69) / 12)


def soft_saw(f, t, harmonics=10, bright=3.0):
    """A band-limited saw with its upper harmonics rolled off, like a low-passed synth."""
    out = np.zeros_like(t)
    for k in range(1, harmonics + 1):
        if f * k > SR / 2.5:
            break
        out += np.sin(2 * np.pi * f * k * t) / k * np.exp(-k / bright)
    return out


def env_adsr(n, attack, release, total):
    """Linear attack, flat, linear release, in samples of a `total`-second note."""
    t = times(n)
    e = np.minimum(1, t / max(attack, 1e-4))
    return e * np.clip((total - t) / max(release, 1e-4), 0, 1)


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
    n = int((total + 3) * SR)
    left = np.zeros(n)
    right = np.zeros(n)
    pad = np.zeros(n)
    bass = np.zeros(n)
    drums = np.zeros(n)
    rng = np.random.default_rng(7)

    groove_from = a.intro
    groove_to = total - a.outro
    final_at = total - a.outro + 0.4  # the held last chord

    bar = 0
    while bar * BAR < total:
        t0 = bar * BAR
        chord = CHORDS[bar % 4] if t0 < final_at else CHORDS[0]
        last = t0 >= final_at
        length = (total - t0 + 1.5) if last else BAR + 0.9
        # Pad: slow attack, overlapping release.
        m = int(length * SR)
        t = times(m)
        e = env_adsr(m, 0.6, 1.2 if not last else total - t0, length)
        for note in chord["pad"]:
            f = hz(note)
            # A centre voice plus two quieter, unevenly detuned ones: chorus without the
            # whole chord beating to silence.
            ph = rng.uniform(0, 1, 3)
            voices = soft_saw(f, t + ph[0]) + 0.5 * soft_saw(f * 1.0011, t + ph[1]) + 0.5 * soft_saw(f * 0.9984, t + ph[2])
            add(pad, t0, voices * e, 0.045)
        if last:
            break
        # Pluck arpeggio, eighth notes, with a stereo echo.
        for i, idx in enumerate(ARP_PATTERN):
            start = t0 + i * BEAT / 2
            if start >= total:
                break
            m2 = int(0.7 * SR)
            tt = times(m2)
            f = hz(chord["arp"][idx])
            tone = (np.sin(2 * np.pi * f * tt) + 0.25 * np.sin(4 * np.pi * f * tt)) * np.exp(-tt * 7)
            tone *= np.minimum(1, tt / 0.004)
            g = 0.07 if start < groove_from else 0.085
            add(left, start, tone, g)
            add(right, start, tone, g)
            add(left, start + BEAT * 0.75, tone, g * 0.35)
            add(right, start + BEAT * 1.5, tone, g * 0.2)
        # Groove: sub bass on the beats, kick on every beat, hats on the off-beats.
        if groove_from <= t0 < groove_to:
            for beat in range(4):
                start = t0 + beat * BEAT
                if start < groove_from or start >= groove_to:
                    continue
                m3 = int(BEAT * 0.95 * SR)
                tt = times(m3)
                f = hz(chord["bass"])
                b = (np.sin(2 * np.pi * f * tt) + 0.3 * np.sin(4 * np.pi * f * tt)) * env_adsr(m3, 0.012, 0.15, BEAT * 0.95)
                add(bass, start, b, 0.12)
                mk = int(0.35 * SR)
                tk = times(mk)
                sweep = 2 * np.pi * np.cumsum(50 + 110 * np.exp(-tk * 35)) / SR
                add(drums, start, np.sin(sweep) * np.exp(-tk * 10), 0.24)
                if t0 >= groove_from + 2 * BAR:
                    mh = int(0.06 * SR)
                    noise = np.diff(rng.standard_normal(mh + 1))
                    add(drums, start + BEAT / 2, noise * np.exp(-times(mh) * 70), 0.025)
        bar += 1

    # Sidechain: the pad and bass dip under each kick.
    duck = np.ones(n)
    b = groove_from
    while b < groove_to:
        if b >= groove_from:
            i = int(b * SR)
            m = int(BEAT * SR)
            seg = 1 - 0.45 * np.exp(-times(m) * 9)
            duck[i : i + m] = np.minimum(duck[i : i + m], seg[: n - i])
        b += BEAT
    duck.flags.writeable = False
    bed = pad * duck + bass * duck + drums
    left += bed
    right += bed
    mix = np.stack([left, right], axis=1)[: int(total * SR)]

    # Fade in, fade out, soft clip, normalize.
    fade_in = int(0.8 * SR)
    mix[:fade_in] *= np.linspace(0, 1, fade_in)[:, None]
    fade_out = int(2.5 * SR)
    mix[-fade_out:] *= np.linspace(1, 0, fade_out)[:, None] ** 1.5
    mix = np.tanh(mix * 1.2)
    mix /= np.max(np.abs(mix)) / 0.89
    pcm = (mix * 32767).astype("<i2")
    with wave.open(a.out, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


if __name__ == "__main__":
    main()
