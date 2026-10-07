"""Variant B of the showreel's soundtrack: a laid-back 96 bpm groove (electric
piano, sub bass, swung drums) with rounder, woodier effects.
usage: python3 sound_b.py out.wav"""
import sys, wave
import numpy as np

SR = 48000
DUR = 15.0
N = int(SR * DUR)
rng = np.random.default_rng(11)
dry = np.zeros((N, 2))     # also feeds the reverb
direct = np.zeros((N, 2))  # drums, bass, key ticks: no reverb

def put(buf, t, sig, gain=1.0, pan=0.0):
    i = int(round(t * SR))
    if i >= N:
        return
    if i < 0:
        sig = sig[-i:]; i = 0
    sig = sig[: N - i]
    l, r = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    buf[i : i + len(sig), 0] += sig * gain * l
    buf[i : i + len(sig), 1] += sig * gain * r

def tt(d):
    return np.arange(int(d * SR)) / SR

def hz(note):
    return 440.0 * 2 ** ((note - 69) / 12)

def band(x, lo, hi):
    """Keeps lo..hi Hz of x."""
    spec = np.fft.rfft(x); f = np.fft.rfftfreq(len(x), 1 / SR)
    spec[(f < lo) | (f > hi)] = 0
    return np.fft.irfft(spec, len(x))

def noise(d):
    return rng.standard_normal(int(d * SR))

def rhodes(note, d=1.2, vel=1.0):
    t = tt(d); f = hz(note)
    tine = np.sin(2 * np.pi * f * 14 * t) * np.exp(-t * 40) * 0.35 * vel
    body = np.sin(2 * np.pi * f * t + np.sin(2 * np.pi * f * t) * 1.1 * vel * np.exp(-t * 3) + tine)
    env = np.exp(-t * 2.4) * (1 - np.exp(-t * 700)) * np.minimum(1, (d - t) / 0.05)
    return body * env * (1 + 0.12 * np.sin(2 * np.pi * 4.6 * t))

def marimba(note, d=0.5):
    t = tt(d); f = hz(note)
    return (np.sin(2 * np.pi * f * t) + 0.3 * np.sin(2 * np.pi * f * 4 * t) * np.exp(-t * 40)) * np.exp(-t * 11) * (1 - np.exp(-t * 2000))

def sub(note, d):
    t = tt(d); f = hz(note)
    s = np.sin(2 * np.pi * f * t) + 0.22 * np.sin(2 * np.pi * 2 * f * t)
    return np.tanh(s * 1.4) * (1 - np.exp(-t * 300)) * np.minimum(1, (d - t) / 0.06) * np.exp(-t * 1.2)

def sweep(f0, f1, d, decay=8.0, curve=1.0):
    t = tt(d); f = f0 * (f1 / f0) ** ((t / d) ** curve)
    return np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t * decay) * (1 - np.exp(-t * 1200))

def bloop(f=420, up=1.9, d=0.16):
    return sweep(f, f * up, d, decay=16, curve=0.5)

def kick():
    return np.tanh(sweep(150, 45, 0.36, decay=9, curve=0.3) * 1.8)

def snare():
    t = tt(0.22)
    return band(noise(0.22), 900, 7500) * np.exp(-t * 22) * 0.8 + np.sin(2 * np.pi * 190 * t) * np.exp(-t * 30) * 0.7

def hat(open_=False):
    d = 0.16 if open_ else 0.045
    t = tt(d)
    return band(noise(d), 6500, 15000) * np.exp(-t * (22 if open_ else 95))

def riser(d=0.9):
    """A swell of air that ends on the cut."""
    t = tt(d)
    return band(noise(d), 1800, 11000) * (t / d) ** 3.2 * np.minimum(1, (d - t) / 0.02)

def thump():
    return sweep(95, 38, 0.5, decay=6, curve=0.3)

def wood():
    t = tt(0.05)
    return np.sin(2 * np.pi * 950 * t) * np.exp(-t * 130) + band(noise(0.05), 1500, 6000) * np.exp(-t * 500) * 0.5

def harp(notes, gap=0.05, d=1.4):
    out = np.zeros(int((d + gap * len(notes)) * SR))
    for i, n in enumerate(notes):
        t = tt(d); f = hz(n)
        s = (np.sin(2 * np.pi * f * t) + 0.35 * np.sin(2 * np.pi * 2 * f * t) * np.exp(-t * 7) + 0.15 * np.sin(2 * np.pi * 3 * f * t) * np.exp(-t * 12)) * np.exp(-t * 3.6) * (1 - np.exp(-t * 1500))
        k = int(i * gap * SR); out[k : k + len(s)] += s
    return out

# ---------------- music: 96 bpm, six bars of 4/4, swung ----------------
BEAT = 0.625
BAR = 4 * BEAT
SWING = 0.085  # how late the off-beat eighth lands
def at(bar, beat):  # beat counted from 0, halves are swung
    late = SWING if abs(beat * 2 % 2 - 1) < 1e-6 else 0.0
    return bar * BAR + beat * BEAT + late

CH = [("Fmaj7", 41, [57, 60, 64, 67]), ("Em7", 40, [55, 59, 62, 66]), ("Dm9", 38, [53, 57, 60, 64]), ("Em7", 40, [55, 59, 62, 67]), ("Fmaj9", 41, [57, 60, 64, 67, 72]), ("Cmaj9", 36, [52, 55, 59, 62, 67])]
for bar, (_, root, notes) in enumerate(CH):
    last = bar == 5
    stabs = [(0, 1.5, 1.0)] if last else [(0, 1.3, 1.0), (1.5, 0.9, 0.7), (3, 0.6, 0.55)]
    for beat, d, vel in stabs:
        for j, n in enumerate(notes):
            put(dry, at(bar, beat) + j * (0.018 if last else 0.006), rhodes(n, 2.6 if last else d, vel), 0.17 * vel, pan=-0.35 + 0.23 * j)
    if bar >= 1:
        hits = [(0, 2.4)] if last else [(0, 0.8), (1.5, 0.5), (2.5, 0.45), (3.5, 0.3)]
        for beat, d in hits:
            put(direct, at(bar, beat), sub(root if beat != 2.5 else root + 7, d), 0.36)

# drums: in from bar 1, out for the last bar
for bar in range(1, 5):
    for beat in (0, 2.5):
        put(direct, at(bar, beat), kick(), 0.62)
    if bar in (2, 4):
        put(direct, at(bar, 3.5), kick(), 0.4)
    for beat in (1, 3):
        put(direct, at(bar, beat), snare(), 0.34); put(dry, at(bar, beat), snare(), 0.07)
    for e in range(8):
        put(direct, at(bar, e / 2), hat(open_=(e == 7 and bar % 2 == 0)), 0.11 if e % 2 == 0 else 0.07, pan=0.3)
    put(direct, at(bar, 1.75), hat(), 0.035, pan=-0.3); put(direct, at(bar, 3.75), hat(), 0.035, pan=-0.3)
# a pick-up into the groove, and one into the outro
for i, off in enumerate((-0.47, -0.31, -0.16)):
    put(direct, BAR + off, snare(), 0.10 + 0.07 * i)
put(direct, 5 * BAR, kick(), 0.5)

# a small marimba figure over bars 2 to 4
motif = [(2, 0, 76), (2, 0.5, 79), (2, 1.5, 76), (2, 2.5, 74), (3, 0, 71), (3, 0.5, 74), (3, 1.5, 79), (3, 3, 78), (4, 0, 76), (4, 0.5, 79), (4, 1.5, 84), (4, 2.5, 83), (4, 3.5, 79)]
for bar, beat, n in motif:
    put(dry, at(bar, beat), marimba(n, 0.6), 0.2, pan=0.25)

# ---------------- effects, on the picture's own times ----------------
PENTA = [72, 74, 76, 79, 81, 84, 86, 88]
put(dry, 0.25, bloop(300, 2.3, 0.2), 0.42)                      # mascot pops in
put(dry, 1.08, bloop(360, 1.7, 0.14), 0.28)                     # first hop
put(dry, 1.0, harp([64, 67, 72, 76, 79], 0.04), 0.16)           # wordmark
for cut, pan in ((2.7, 0.4), (6.0, -0.4), (11.25, 0.4), (13.45, 0.0)):   # scene changes
    put(dry, cut - 0.9, riser(0.9), 0.20, pan=pan)
    put(direct, cut, thump(), 0.34)
for i in range(3):                                              # onboarding cards land
    put(dry, 2.97 + i * 0.11, wood(), 0.22, pan=0.2 + 0.2 * i)
for i, t0 in enumerate((3.68, 4.73, 5.73)):                     # Continue, Continue, Start
    put(direct, t0, wood(), 0.36, pan=0.35)
    put(dry, t0 + 0.04, bloop(520 + 90 * i, 1.5, 0.12), 0.2, pan=0.35)
put(direct, 7.02, wood(), 0.36, pan=0.4)                        # click on the mascot
put(dry, 7.06, bloop(240, 3.0, 0.3), 0.38, pan=0.3)             # panel springs open
put(dry, 7.12, harp([67, 72, 76], 0.035, 0.9), 0.12, pan=0.3)
# typing: every key is a quiet marimba note, on reel.html's own schedule
SENT = "Where is the nearest train station?"
ct = []; x = 7.75
for i in range(len(SENT)):
    x += 0.034 + ((i * 7919) % 13) / 13 * 0.018 + (0.03 if i > 0 and SENT[i - 1] == " " else 0)
    ct.append(x)
for i, t0 in enumerate(ct):
    if SENT[i] == " ":
        put(direct, t0, wood(), 0.10, pan=0.3)
    else:
        put(direct, t0, marimba(PENTA[(i * 5 + i // 3) % len(PENTA)] + 12, 0.12), 0.085, pan=0.3 + 0.1 * rng.standard_normal())
        put(direct, t0, band(noise(0.02), 2500, 9000) * np.exp(-tt(0.02) * 600), 0.05, pan=0.3)
put(dry, ct[7] + 0.32, bloop(600, 1.25, 0.1), 0.13, pan=0.3)    # partial translations
put(dry, ct[19] + 0.32, bloop(680, 1.25, 0.1), 0.13, pan=0.3)
done = ct[-1] + 0.42                                            # the translation lands
put(dry, done, harp([72, 76, 79, 84, 88, 91], 0.045, 1.8), 0.24, pan=0.25)
put(dry, done + 0.13, bloop(340, 1.9, 0.16), 0.26, pan=0.4)     # the happy hop
for i, t0 in enumerate((12.1, 12.5, 12.9, 13.25, 13.65)):       # one colour, one bloop
    put(dry, t0, bloop(380 + 70 * i, 1.6 + 0.1 * i, 0.14), 0.3, pan=-0.4 + 0.2 * i)
    put(dry, t0, marimba(PENTA[i + 2], 0.5), 0.16, pan=-0.4 + 0.2 * i)
put(dry, 13.75, harp([60, 64, 67, 71, 74, 79, 83, 86], 0.05, 2.4), 0.22)   # outro wordmark
put(dry, 14.13, bloop(340, 2.0, 0.18), 0.28)                    # last hop

# ---------------- reverb, master ----------------
ir_t = tt(1.0)
ir = rng.standard_normal((len(ir_t), 2)) * np.exp(-ir_t * 5.5)[:, None]
ir[: int(0.01 * SR)] = 0
size = 1 << int(np.ceil(np.log2(N + len(ir))))
wet = np.stack([np.fft.irfft(np.fft.rfft(dry[:, c], size) * np.fft.rfft(ir[:, c], size), size)[:N] for c in range(2)], axis=1)
wet /= np.abs(wet).max() + 1e-9
mix = dry + direct + wet * 0.38 * np.abs(dry).max()
mix /= np.abs(mix).max()
mix = np.tanh(mix * 1.6) / np.tanh(1.6)
i = np.arange(N)
mix = mix * (np.minimum(1, i / (0.02 * SR)) * np.minimum(1, (N - i) / (0.6 * SR)) ** 1.5)[:, None] * 0.89
with wave.open(sys.argv[1], "wb") as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR)
    w.writeframes((mix * 32767).astype("<i2").tobytes())
