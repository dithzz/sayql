"""An original background track for the explainer, synthesized from scratch (no samples, no
third-party music), so there's nothing to license.

    python3 scripts/video/music.py OUT.wav DURATION [--intro SECONDS] [--outro SECONDS]

A clean, laid-back "coding" groove at 92 BPM: electric-piano chords (Dm9 – G13 – Cmaj9 – Am9),
a round bass, a soft kick and snare, swung hats and a small hook every few bars, with a touch
of reverb. The title card (`--intro`) gets the piano alone; the groove starts as the app
appears, and stops on a final Cmaj9 as the end card comes in (`--outro` seconds before the
end). Needs only numpy.
"""

import argparse
import wave

import numpy as np

SR = 44100
BPM = 92
BEAT = 60 / BPM
BAR = 4 * BEAT
S16 = BEAT / 4
SWING = 0.16 * S16  # the off-beat 16ths come a little late

# Rootless jazz voicings for the piano, and the bass root (MIDI).
CHORDS = [
    {"keys": [53, 57, 60, 64], "bass": 38},  # Dm9
    {"keys": [53, 57, 59, 64], "bass": 43},  # G13
    {"keys": [52, 55, 59, 62], "bass": 36},  # Cmaj9
    {"keys": [55, 59, 60, 64], "bass": 45},  # Am9
]
FINAL = {"keys": [52, 55, 59, 62, 67], "bass": 36}  # Cmaj9

KEYS_HITS = [(0, 6, 1.0), (7, 4, 0.7), (12, 4, 0.45)]  # (16th, length in 16ths, velocity)
BASS_HITS = [(0, 7), (7, 3), (10, 6)]
KICKS = [0, 10]
SNARES = [4, 12]
HAT_VEL = [0.9, 0.35, 0.6, 0.35]
# The hook, over bars 3 and 4 of every four (Cmaj9, then Am9): (16th, MIDI).
HOOK = [[(0, 76), (3, 79), (6, 81), (10, 79), (14, 76)], [(0, 74), (3, 76), (8, 72)]]


def ro(a):
    """Marks `a` read-only. numpy 2.2 on Python 3.14 can otherwise reuse a named array as
    scratch space in `a * x` and similar, silently changing it."""
    a.flags.writeable = False
    return a


def times(n):
    return ro(np.arange(n) / SR)


def hz(midi):
    return 440.0 * 2 ** ((midi - 69) / 12)


def at16(bar_start, step):
    return bar_start + step * S16 + (SWING if step % 2 else 0)


def fft_filter(x, lo=None, hi=None):
    """Zero-phase band filter with soft edges, done in the frequency domain."""
    spec = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / SR)
    gain = np.ones_like(f)
    if lo:
        gain = gain * (1 / (1 + (lo / np.maximum(f, 1)) ** 4))
    if hi:
        gain = gain * (1 / (1 + (f / hi) ** 4))
    return np.fft.irfft(spec * gain, len(x))


class Track:
    def __init__(self, seconds):
        self.n = int(seconds * SR)
        self.dry = np.zeros((self.n, 2))
        self.send = np.zeros((self.n, 2))  # to the reverb
        self.duck = np.zeros((self.n, 2))  # ducked under the kick

    def add(self, bus, start, sig, gain=1.0, pan=0.5):
        i = int(start * SR)
        if i < 0 or i >= self.n:
            return
        sig = sig[: self.n - i]
        bus[i : i + len(sig), 0] += sig * gain * np.sqrt(1 - pan)
        bus[i : i + len(sig), 1] += sig * gain * np.sqrt(pan)


def e_piano(f, length, vel, decay=1.3):
    """An FM electric piano: a bright tine at the attack that mellows as it rings."""
    n = int((length + 0.6) * SR)
    t = times(n)
    index = ro(1.3 * vel * np.exp(-t * 7) + 0.22)
    tone = np.sin(2 * np.pi * f * t + index * np.sin(2 * np.pi * f * t))
    tone = tone + 0.06 * np.sin(2 * np.pi * 4 * f * t) * np.exp(-t * 14)
    env = np.minimum(1, t / 0.003) * np.exp(-t * decay) * np.clip((length + 0.6 - t) / 0.6, 0, 1)
    return tone * env * (1 + 0.07 * np.sin(2 * np.pi * 4.8 * t))


def bass_note(f, length):
    n = int((length + 0.05) * SR)
    t = times(n)
    x = np.sin(2 * np.pi * f * t) + 0.35 * np.sin(4 * np.pi * f * t)
    env = np.minimum(1, t / 0.006) * np.clip((length + 0.05 - t) / 0.05, 0, 1) * (0.75 + 0.25 * np.exp(-t * 6))
    return np.tanh(1.6 * x) * env


def kick():
    t = times(int(0.45 * SR))
    phase = 2 * np.pi * np.cumsum(46 + 75 * np.exp(-t * 32)) / SR
    return np.sin(phase) * np.exp(-t * 7.5) * np.minimum(1, t / 0.0015)


def snare(rng):
    t = times(int(0.35 * SR))
    noise = fft_filter(rng.standard_normal(len(t)), lo=1200, hi=7000)
    noise = noise / np.max(np.abs(noise))
    body = np.sin(2 * np.pi * 185 * t) * np.exp(-t * 28)
    return (0.8 * noise * np.exp(-t * 17) + 0.45 * body) * np.minimum(1, t / 0.001)


def hat(rng, decay):
    t = times(int(0.25 * SR))
    noise = fft_filter(rng.standard_normal(len(t)), lo=7500)
    return noise / np.max(np.abs(noise)) * np.exp(-t * decay)


def pluck(f):
    t = times(int(1.4 * SR))
    x = np.sin(2 * np.pi * f * t) + 0.18 * np.sin(2 * np.pi * 2 * f * t + 0.5) * np.exp(-t * 8)
    return x * np.exp(-t * 4.2) * np.minimum(1, t / 0.004)


def reverb(x, rng, seconds=1.8):
    """Convolution with a decaying, darkened noise tail (a small, soft room)."""
    m = int(seconds * SR)
    t = times(m)
    out = np.zeros_like(x)
    size = 1 << int(np.ceil(np.log2(len(x) + m)))
    for ch in range(2):
        ir = fft_filter(rng.standard_normal(m), hi=5000) * np.exp(-t * 3.4)
        ir = ir / np.sqrt(np.sum(ir**2))
        out[:, ch] = np.fft.irfft(np.fft.rfft(x[:, ch], size) * np.fft.rfft(ir, size), size)[: len(x)]
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("duration", type=float)
    ap.add_argument("--intro", type=float, default=3.0)
    ap.add_argument("--outro", type=float, default=4.0)
    a = ap.parse_args()

    total = a.duration
    groove_at = a.intro
    final_at = total - a.outro
    # The bar grid is anchored so a bar starts exactly when the groove does; one bar of
    # piano before it (it may start before 0, and is cut there).
    origin = groove_at - BAR
    pre = max(0.0, -origin)
    tr = Track(total + pre + 3)
    T = lambda t: t + pre  # song time → buffer time
    rng = np.random.default_rng(11)
    KICK, SNARE = ro(kick()), ro(snare(rng))
    HAT, OPEN_HAT = ro(hat(rng, 60)), ro(hat(rng, 13))
    kick_times = []

    bar = 0
    while origin + bar * BAR < final_at:
        b0 = origin + bar * BAR
        chord = CHORDS[(bar - 1) % 4]  # the groove opens on Dm9; the intro bar is Am9
        groove = b0 >= groove_at - 1e-6
        for step, length, vel in KEYS_HITS:
            start = at16(b0, step)
            if start >= final_at:
                continue
            dur = length * S16
            for k, note in enumerate(chord["keys"]):
                sig = e_piano(hz(note), dur, vel)
                pan = 0.35 + 0.1 * k
                g = 0.08 * vel
                tr.add(tr.duck, T(start), sig, g, pan)
                tr.add(tr.send, T(start), sig, g * 0.5, pan)
        if groove:
            for step, length in BASS_HITS:
                start = at16(b0, step)
                if start < final_at:
                    tr.add(tr.duck, T(start), bass_note(hz(chord["bass"]), length * S16), 0.12)
            for step in KICKS:
                start = at16(b0, step)
                if start < final_at:
                    tr.add(tr.dry, T(start), KICK, 0.5)
                    kick_times.append(T(start))
            for step in SNARES:
                start = at16(b0, step)
                if start < final_at:
                    tr.add(tr.dry, T(start), SNARE, 0.17)
                    tr.add(tr.send, T(start), SNARE, 0.1)
            if b0 >= groove_at + BAR - 1e-6:  # hats from the second bar of the groove
                for step in range(16):
                    start = at16(b0, step)
                    if start >= final_at:
                        continue
                    if step == 14 and bar % 2:
                        tr.add(tr.dry, T(start), OPEN_HAT, 0.05, 0.6)
                    else:
                        tr.add(tr.dry, T(start), HAT, 0.045 * HAT_VEL[step % 4], 0.6)
            cycle = (bar - 1) % 4  # bar 1 is the first groove bar
            if cycle in (2, 3) and b0 >= groove_at + 2 * BAR - 1e-6:
                for step, note in HOOK[cycle - 2]:
                    start = at16(b0, step)
                    if start < final_at - 0.3:
                        sig = pluck(hz(note))
                        tr.add(tr.dry, T(start), sig, 0.05, 0.55)
                        tr.add(tr.dry, T(start + 3 * S16), sig, 0.016, 0.25)  # dotted-16th echo
                        tr.add(tr.send, T(start), sig, 0.04, 0.55)
        bar += 1

    # The end: one last Cmaj9 that rings out, with the bass.
    ring = total - final_at + 1
    for k, note in enumerate(FINAL["keys"]):
        sig = e_piano(hz(note), ring, 0.9, decay=0.45)
        tr.add(tr.duck, T(final_at), sig, 0.08, 0.3 + 0.1 * k)
        tr.add(tr.send, T(final_at), sig, 0.04, 0.3 + 0.1 * k)
    tr.add(tr.duck, T(final_at), bass_note(hz(FINAL["bass"]), 2.0) * np.exp(-times(int(2.05 * SR)) * 1.2), 0.12)

    # Sidechain: piano and bass dip under each kick, for the gentle pump of the genre.
    gain = np.ones(tr.n)
    m = int(BEAT * SR)
    dip = ro(1 - 0.3 * np.exp(-times(m) * 11))
    for kt in kick_times:
        i = int(kt * SR)
        gain[i : i + m] = np.minimum(gain[i : i + m], dip[: max(0, min(m, tr.n - i))])
    mix = tr.dry + tr.duck * ro(gain)[:, None] + 0.28 * reverb(tr.send, rng)

    mix = mix[int(pre * SR) : int((pre + total) * SR)]
    fade_in = int(0.6 * SR)
    mix[:fade_in] *= (np.linspace(0, 1, fade_in) ** 2)[:, None]
    fade_out = int(2.5 * SR)
    mix[-fade_out:] *= (np.linspace(1, 0, fade_out) ** 2)[:, None]
    mix = np.tanh(mix * 1.1)
    mix /= np.max(np.abs(mix)) / 0.89
    pcm = (mix * 32767).astype("<i2")
    with wave.open(a.out, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


if __name__ == "__main__":
    main()
