"""Synthesises the showreel's soundtrack (music bed + effects synced to the picture).
usage: python3 sound.py out.wav"""
import sys, wave
import numpy as np

SR = 48000
DUR = 15.0
N = int(SR * DUR)
rng = np.random.default_rng(7)
dry = np.zeros((N, 2))   # goes to the reverb too
direct = np.zeros((N, 2))  # stays dry (kick, ticks)

def put(buf, t, sig, gain=1.0, pan=0.0):
    i = int(t * SR)
    if i >= N or i < 0:
        return
    sig = sig[: N - i]
    l, r = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    buf[i : i + len(sig), 0] += sig * gain * l
    buf[i : i + len(sig), 1] += sig * gain * r

def tt(d):
    return np.arange(int(d * SR)) / SR

def hz(note):  # midi -> Hz
    return 440.0 * 2 ** ((note - 69) / 12)

def pluck(note, d=0.9, bright=1.0):
    t = tt(d); f = hz(note)
    env = np.exp(-t * 5.5) * (1 - np.exp(-t * 900))
    mod = np.sin(2 * np.pi * f * 3 * t) * np.exp(-t * 18) * 1.6 * bright
    return np.sin(2 * np.pi * f * t + mod) * env

def bell(note, d=1.6):
    t = tt(d); f = hz(note)
    s = np.sin(2 * np.pi * f * t) + 0.45 * np.sin(2 * np.pi * f * 2.76 * t) * np.exp(-t * 6) + 0.25 * np.sin(2 * np.pi * f * 5.4 * t) * np.exp(-t * 12)
    return s * np.exp(-t * 3.2) * (1 - np.exp(-t * 1500))

def pad(notes, d, attack=0.5, release=0.8):
    t = tt(d + release); out = np.zeros_like(t)
    for k, n in enumerate(notes):
        f = hz(n)
        for det in (-0.004, 0.0, 0.005):
            ph = rng.random() * 6.28
            out += np.sin(2 * np.pi * f * (1 + det) * t + ph) + 0.28 * np.sin(2 * np.pi * 2 * f * (1 + det) * t + ph)
    env = np.minimum(t / attack, 1.0) * np.where(t > d, np.exp(-(t - d) * 4.5), 1.0)
    trem = 1 + 0.06 * np.sin(2 * np.pi * 0.7 * t)
    return out / (3 * len(notes)) * env * trem

def sweep(f0, f1, d, decay=8.0, curve=1.0):
    t = tt(d); x = (t / d) ** curve
    f = f0 * (f1 / f0) ** x
    ph = 2 * np.pi * np.cumsum(f) / SR
    return np.sin(ph) * np.exp(-t * decay) * (1 - np.exp(-t * 1200))

def kick():
    return sweep(120, 42, 0.32, decay=11, curve=0.35)

def noise_band(d, f0, f1, q=2.5):
    """White noise through a band-pass whose centre glides from f0 to f1."""
    n = int(d * SR); x = rng.standard_normal(n); out = np.zeros(n)
    lp = bp = 0.0
    for i in range(n):
        f = f0 * (f1 / f0) ** (i / n)
        g = 2 * np.sin(np.pi * min(f, SR / 6.5) / SR)
        hp = x[i] - lp - bp / q
        bp += g * hp
        lp += g * bp
        out[i] = bp
    return out

def whoosh(d=0.75, up=True):
    f0, f1 = (350, 5200) if up else (5200, 350)
    t = tt(d); env = np.sin(np.pi * t / d) ** 2.2
    return noise_band(d, f0, f1)[: len(t)] * env

def click():
    t = tt(0.05)
    return (rng.standard_normal(len(t)) * np.exp(-t * 900) * 0.7 + np.sin(2 * np.pi * 1900 * t) * np.exp(-t * 160) + 0.6 * np.sin(2 * np.pi * 240 * t) * np.exp(-t * 90))

def tick(p):
    t = tt(0.03)
    return rng.standard_normal(len(t)) * np.exp(-t * 700) * 0.5 + np.sin(2 * np.pi * (2300 + 700 * p) * t) * np.exp(-t * 260) * 0.6

def shaker():
    t = tt(0.06); x = rng.standard_normal(len(t)); x = np.diff(x, prepend=0)  # crude high-pass
    return x * np.exp(-t * 85)

# ---------------- music: 120 bpm, D major ----------------
BEAT = 0.5
CH = {"D": [50, 57, 62, 66, 69], "Bm": [47, 54, 59, 62, 66], "G": [43, 50, 59, 62, 67], "A": [45, 52, 57, 61, 64], "Dadd9": [50, 57, 62, 64, 66, 69]}
prog = [(0.0, "D", 3.0), (3.0, "Bm", 2.0), (5.0, "G", 1.0), (6.0, "D", 2.0), (8.0, "Bm", 2.0), (10.0, "G", 1.25), (11.25, "A", 2.25), (13.5, "Dadd9", 1.5)]
for at, name, d in prog:
    put(dry, at, pad(CH[name], d, attack=0.9 if at == 0 else 0.25), 0.24)
    put(dry, at, pad([CH[name][0] - 12], d, attack=0.2), 0.30)  # bass

def chord_at(t):
    cur = prog[0][1]
    for at, name, _ in prog:
        if t >= at:
            cur = name
    return CH[cur]

# arpeggio: eighth notes, entering after the wordmark, dropping out for the outro
pattern = [1, 2, 3, 4, 3, 2, 4, 3]
k = 0
t = 1.0
while t < 13.45:
    notes = chord_at(t)
    n = notes[pattern[k % 8] % len(notes)] + 12
    level = 0.20 if t < 2.75 else 0.30
    if 11.25 <= t:
        level = 0.36
    put(dry, t, pluck(n, bright=0.7 + 0.5 * (k % 2)), level, pan=0.5 * np.sin(k * 1.3))
    if t >= 6.0 and k % 4 == 0:
        put(dry, t, pluck(n + 12, 0.6, 0.4), 0.10, pan=-0.4)
    t += BEAT / 2; k += 1

# pulse: soft kick on the beat and a shaker on the off-beats, from the onboarding on
t = 3.0
while t < 13.4:
    put(direct, t, kick(), 0.5)
    put(direct, t + BEAT / 2, shaker(), 0.05 if t < 6 else 0.085, pan=0.3)
    if t >= 6.0:
        put(direct, t + BEAT * 0.75, shaker(), 0.04, pan=-0.3)
    t += BEAT

# ---------------- effects, on the picture's own times ----------------
# mascot pops in, then hops
put(dry, 0.25, sweep(260, 780, 0.22, decay=13, curve=0.6), 0.5)
put(dry, 0.25, bell(81, 1.2), 0.12)
put(dry, 1.08, sweep(300, 620, 0.16, decay=12), 0.32)
put(dry, 1.0, bell(86, 1.4), 0.16); put(dry, 1.0, bell(74, 1.6), 0.14)   # wordmark
# scene changes
for at, up, g in [(2.55, True, 0.30), (5.85, False, 0.28), (11.05, True, 0.32), (13.3, False, 0.26)]:
    put(dry, at, whoosh(0.8, up), g, pan=0.5 if up else -0.5)
# onboarding cards land
for i in range(3):
    put(dry, 2.95 + i * 0.11, sweep(190 + 40 * i, 120, 0.12, decay=22), 0.22, pan=0.4)
# clicks: three Continue buttons, then the mascot
for at in (3.68, 4.73, 5.73, 7.02):
    put(direct, at, click(), 0.42, pan=0.35)
for i, at in enumerate((3.72, 4.77, 5.77)):
    put(dry, at, pluck([74, 78, 81][i] + 12, 0.5, 0.5), 0.2, pan=0.35)
# panel springs open
put(dry, 7.06, sweep(220, 900, 0.3, decay=9, curve=0.5), 0.36, pan=0.3)
put(dry, 7.1, bell(78, 1.0), 0.12, pan=0.3)
# typing: the same schedule as reel.html
SENT = "Where is the nearest train station?"
ct = []; x = 7.75
for i in range(len(SENT)):
    x += 0.034 + ((i * 7919) % 13) / 13 * 0.018 + (0.03 if i > 0 and SENT[i - 1] == " " else 0)
    ct.append(x)
for i, at in enumerate(ct):
    put(direct, at, tick(rng.random()), 0.16 if SENT[i] != " " else 0.22, pan=0.25 + 0.1 * rng.standard_normal())
# partial translations arrive, then the full one
put(dry, ct[7] + 0.32, pluck(86, 0.4, 0.3), 0.14, pan=0.3)
put(dry, ct[19] + 0.32, pluck(88, 0.4, 0.3), 0.14, pan=0.3)
done = ct[-1] + 0.42
put(dry, done, bell(81, 1.6), 0.26, pan=0.2); put(dry, done + 0.11, bell(86, 1.8), 0.28, pan=0.3); put(dry, done + 0.22, bell(90, 2.0), 0.22, pan=0.4)
put(dry, done + 0.13, sweep(300, 640, 0.16, decay=12), 0.26, pan=0.4)   # the happy hop
# colours: one rising note each
for i, at in enumerate((12.1, 12.5, 12.9, 13.25, 13.65)):
    put(dry, at, pluck([81, 83, 86, 88, 93][i], 0.7, 1.2), 0.26, pan=-0.4 + 0.2 * i)
    put(dry, at, sweep(500 + 90 * i, 900 + 120 * i, 0.09, decay=20), 0.12)
# outro: wordmark and last hop
for i, n in enumerate((62, 69, 74, 78, 81, 86)):
    put(dry, 13.75 + i * 0.045, bell(n, 2.2), 0.17, pan=-0.5 + 0.2 * i)
put(dry, 14.13, sweep(300, 660, 0.18, decay=11), 0.28)

# ---------------- reverb, master ----------------
ir_t = tt(1.3)
ir = rng.standard_normal((len(ir_t), 2)) * np.exp(-ir_t * 4.2)[:, None]
ir[: int(0.012 * SR)] = 0
size = 1 << int(np.ceil(np.log2(N + len(ir))))
wet = np.stack([np.fft.irfft(np.fft.rfft(dry[:, c], size) * np.fft.rfft(ir[:, c], size), size)[:N] for c in range(2)], axis=1)
wet /= np.abs(wet).max() + 1e-9
mix = dry + direct + wet * 0.5 * np.abs(dry).max()
mix /= np.abs(mix).max()
mix = np.tanh(mix * 1.5) / np.tanh(1.5)
fade = np.minimum(1, np.arange(N) / (0.03 * SR)) * np.minimum(1, (N - np.arange(N)) / (0.7 * SR)) ** 1.5
mix = mix * fade[:, None] * 0.89
with wave.open(sys.argv[1], "wb") as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR)
    w.writeframes((mix * 32767).astype("<i2").tobytes())
