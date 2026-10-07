"""Variant C of the showreel's soundtrack: an upbeat 120 bpm disco-house track
(four-on-the-floor, off-beat bass, pumping chord stabs) with bright, glassy effects.
usage: python3 sound_c.py out.wav"""
import sys, wave
import numpy as np

SR = 48000
DUR = 15.0
N = int(SR * DUR)
rng = np.random.default_rng(23)
music = np.zeros((N, 2))   # chords, bass, lead: ducked by the kick, sent to the reverb
drums = np.zeros((N, 2))
fx = np.zeros((N, 2))      # effects: sent to the reverb
ticks = np.zeros((N, 2))   # clicks and keys: dry

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
    spec = np.fft.rfft(x); f = np.fft.rfftfreq(len(x), 1 / SR)
    spec[(f < lo) | (f > hi)] = 0
    return np.fft.irfft(spec, len(x))

def noise(d):
    return rng.standard_normal(int(d * SR))

def saw(f, t, harmonics=14, tilt=1.0, close=0.0):
    """A saw built from its harmonics; `close` (per second) shuts the top ones over time."""
    out = np.zeros_like(t)
    for h in range(1, harmonics + 1):
        if f * h > SR / 2.2:
            break
        out += np.sin(2 * np.pi * f * h * t + h) / h ** tilt * np.exp(-t * close * (h - 1))
    return out

def stab(notes, d=0.32, bright=1.0):
    t = tt(d + 0.25); out = np.zeros_like(t)
    for n in notes:
        for det in (-0.006, 0.0, 0.007):
            out += saw(hz(n) * (1 + det), t, 12, 1.15 / bright ** 0.2, close=2.2)
    env = (1 - np.exp(-t * 900)) * np.where(t < d, np.exp(-t * 3.5), np.exp(-d * 3.5) * np.exp(-(t - d) * 22))
    return out / (3 * len(notes)) * env

def bass(note, d=0.21):
    t = tt(d); f = hz(note)
    s = saw(f, t, 9, 1.0, close=5.0) * 0.7 + np.sin(2 * np.pi * f * t)
    return np.tanh(s * 1.3) * (1 - np.exp(-t * 700)) * np.minimum(1, (d - t) / 0.03)

def pluck(note, d=0.35):
    t = tt(d); f = hz(note)
    return (np.sin(2 * np.pi * f * t) + 0.5 * saw(f, t, 6, 1.4, close=9)) * np.exp(-t * 13) * (1 - np.exp(-t * 2500))

def glass(note, d=1.2):
    t = tt(d); f = hz(note)
    return (np.sin(2 * np.pi * f * t) + 0.3 * np.sin(2 * np.pi * f * 3.01 * t) * np.exp(-t * 9) + 0.2 * np.sin(2 * np.pi * f * 4.2 * t) * np.exp(-t * 14)) * np.exp(-t * 4.5) * (1 - np.exp(-t * 3000))

def sparkle(notes, gap=0.035):
    out = np.zeros(int((0.9 + gap * len(notes)) * SR))
    for i, n in enumerate(notes):
        s = glass(n, 0.9); k = int(i * gap * SR); out[k : k + len(s)] += s
    return out

def sweep(f0, f1, d, decay=8.0, curve=1.0):
    t = tt(d); f = f0 * (f1 / f0) ** ((t / d) ** curve)
    return np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t * decay) * (1 - np.exp(-t * 1200))

def zap(f=500, up=2.2, d=0.14):
    return sweep(f, f * up, d, decay=18, curve=0.45)

def kick():
    t = tt(0.3)
    return np.tanh(sweep(170, 48, 0.3, decay=8.5, curve=0.28) * 2.2) + band(noise(0.3), 2000, 8000) * np.exp(-t * 400) * 0.25

def clap():
    t = tt(0.2); x = band(noise(0.2), 1100, 9000)
    env = np.exp(-t * 26) + sum(np.exp(-np.maximum(t - o, 0) * 300) * (t >= o) for o in (0.0, 0.011, 0.023)) * 0.6
    return x * env * 0.6

def hat(open_=False):
    d = 0.2 if open_ else 0.04; t = tt(d)
    return band(noise(d), 7000, 16000) * np.exp(-t * (16 if open_ else 110))

def riser(d):
    t = tt(d); x = noise(d)
    return (band(x, 600, 2500) * (1 - t / d) + band(x, 3000, 13000) * (t / d)) * (t / d) ** 2.4 * np.minimum(1, (d - t) / 0.015)

def crash(d=1.4):
    t = tt(d)
    return band(noise(d), 4500, 15000) * np.exp(-t * 4.2)

def click():
    t = tt(0.04)
    return np.sin(2 * np.pi * 2600 * t) * np.exp(-t * 220) + band(noise(0.04), 3000, 12000) * np.exp(-t * 700) * 0.6

# ---------------- music: 120 bpm ----------------
BEAT = 0.5
CHORDS = [(0.0, 45, [57, 60, 64, 67]), (2.0, 41, [57, 60, 65, 69]), (4.0, 48, [55, 60, 64, 67]), (6.0, 43, [55, 59, 62, 67]),
          (8.0, 45, [57, 60, 64, 67, 71]), (10.0, 41, [57, 60, 65, 69, 72]), (12.0, 43, [55, 59, 62, 67, 71])]
def chord_at(t):
    cur = CHORDS[0]
    for c in CHORDS:
        if t >= c[0] - 1e-6:
            cur = c
    return cur

GROOVE0, BREAK0, DROP, END = 3.0, 10.75, 11.25, 13.5
# stabs: sparse and soft in the intro, the classic off-beat push once the kick is in
t = 0.0
while t < END - 1e-6:
    _, _, notes = chord_at(t)
    step = int(round(t / 0.25)) % 8
    if t < GROOVE0:
        if step in (0, 3):
            put(music, t, stab(notes, 0.5, 0.6), 0.34 + 0.05 * t)
    elif not (BREAK0 <= t < DROP):
        if step in (0, 3, 6):
            put(music, t, stab(notes, 0.26, 1.0), 0.30, pan=(-0.25, 0.25, 0.0)[(0, 3, 6).index(step)])
        if t >= DROP and step in (2, 5):
            put(music, t, stab([n + 12 for n in notes[:3]], 0.12, 1.2), 0.14, pan=0.4)
    t += 0.25
# bass: root on the beat's off-eighth, an octave jump at the end of each bar
t = GROOVE0
while t < END - 1e-6:
    if not (BREAK0 <= t < DROP):
        _, root, _ = chord_at(t)
        eighth = int(round(t / 0.25)) % 8
        if eighth % 2 == 1 or eighth == 0:
            put(music, t, bass(root - 12 + (12 if eighth == 7 else 0), 0.2 if eighth else 0.3), 0.42)
    t += 0.25
# lead: a sixteenth-note arpeggio over the last section
t = DROP; k = 0
while t < END - 1e-6:
    _, _, notes = chord_at(t)
    n = notes[(0, 2, 1, 3, 2, 4, 3, 1)[k % 8] % len(notes)] + 24
    put(music, t, pluck(n), 0.15, pan=0.6 * np.sin(k * 0.9))
    t += 0.125; k += 1
# final chord
for j, n in enumerate([48, 55, 60, 64, 67, 74, 76]):
    put(music, END + j * 0.012, stab([n], 1.5, 0.8) * np.exp(-tt(1.75) * 0.9)[: len(stab([n], 1.5, 0.8))], 0.20, pan=-0.5 + j * 0.16)
put(music, END, np.sin(2 * np.pi * hz(36) * tt(1.6)) * np.exp(-tt(1.6) * 2.2), 0.5)

# the kick ducks the music
duck = np.ones(N)
t = GROOVE0
kicks = []
while t < END - 1e-6:
    if not (BREAK0 <= t < DROP):
        kicks.append(t)
    t += BEAT
kicks.append(END)
for k0 in kicks:
    i = int(k0 * SR); n = int(0.42 * SR); seg = tt(0.42)[: max(0, min(n, N - i))]
    duck[i : i + len(seg)] = np.minimum(duck[i : i + len(seg)], 1 - 0.62 * np.exp(-seg * 9))
music *= duck[:, None]

# drums
for k0 in kicks:
    put(drums, k0, kick(), 0.74)
for k0 in kicks[:-1]:
    put(drums, k0 + BEAT / 2, hat(open_=True), 0.10, pan=0.25)
    put(drums, k0 + 0.125, hat(), 0.04, pan=-0.3); put(drums, k0 + 0.375, hat(), 0.05, pan=-0.3)
    if int(round(k0 / BEAT)) % 2 == 1:
        put(drums, k0, clap(), 0.30); put(fx, k0, clap(), 0.06)
for i, off in enumerate((0.0, 0.125, 0.25, 0.3125, 0.375, 0.4375)):   # snare roll through the break
    put(drums, BREAK0 + off, clap(), 0.10 + 0.035 * i)
put(fx, GROOVE0 - 1.5, riser(1.5), 0.22); put(fx, GROOVE0, crash(), 0.16)
put(fx, DROP - 1.1, riser(1.1), 0.26); put(fx, DROP, crash(), 0.20)
put(fx, END - 0.5, riser(0.5), 0.14); put(fx, END, crash(1.6), 0.18)

# ---------------- effects, on the picture's own times ----------------
put(fx, 0.25, zap(320, 2.6, 0.2), 0.42)                                    # mascot pops in
put(fx, 0.3, sparkle([84, 88, 91, 96]), 0.10)
put(fx, 1.08, zap(420, 1.8, 0.13), 0.26)                                   # first hop
put(fx, 1.0, sparkle([76, 81, 84, 88, 93], 0.04), 0.15)                    # wordmark
for cut, pan in ((6.0, -0.4), (13.45, 0.0)):                               # scene changes without a build of their own
    put(fx, cut - 0.6, riser(0.6), 0.16, pan=pan)
for i in range(3):                                                         # onboarding cards land
    put(fx, 2.97 + i * 0.11, zap(700 + 150 * i, 0.6, 0.09), 0.16, pan=0.2 + 0.2 * i)
for i, t0 in enumerate((3.68, 4.73, 5.73)):                                # Continue, Continue, Start
    put(ticks, t0, click(), 0.34, pan=0.35)
    put(fx, t0 + 0.03, glass(88 + (0, 3, 7)[i], 0.6), 0.16, pan=0.35)
put(ticks, 7.02, click(), 0.34, pan=0.4)                                   # click on the mascot
put(fx, 7.06, zap(260, 3.4, 0.28), 0.36, pan=0.3)                          # panel springs open
put(fx, 7.1, sparkle([81, 84, 88, 93], 0.03), 0.13, pan=0.3)
SENT = "Where is the nearest train station?"                               # typing, on reel.html's schedule
ct = []; x = 7.75
for i in range(len(SENT)):
    x += 0.034 + ((i * 7919) % 13) / 13 * 0.018 + (0.03 if i > 0 and SENT[i - 1] == " " else 0)
    ct.append(x)
for i, t0 in enumerate(ct):
    d = 0.022; e = tt(d)
    key = band(noise(d), 3500, 12000) * np.exp(-e * 420) + np.sin(2 * np.pi * (3200 + 900 * rng.random()) * e) * np.exp(-e * 380) * 0.5
    put(ticks, t0, key, 0.13 if SENT[i] != " " else 0.18, pan=0.3 + (0.25 if i % 2 else -0.05))
put(fx, ct[7] + 0.32, glass(93, 0.5), 0.11, pan=0.3)                        # partial translations
put(fx, ct[19] + 0.32, glass(96, 0.5), 0.11, pan=0.3)
done = ct[-1] + 0.42                                                       # the translation lands
put(fx, done, sparkle([81, 84, 88, 93, 96, 100], 0.04), 0.24, pan=0.25)
put(fx, done, stab([69, 72, 76, 81], 0.5, 1.2), 0.18, pan=0.25)
put(fx, done + 0.13, zap(380, 2.0, 0.15), 0.24, pan=0.4)                   # the happy hop
for i, t0 in enumerate((12.1, 12.5, 12.9, 13.25, 13.65)):                  # one colour, one note
    put(fx, t0, glass([88, 91, 93, 96, 100][i], 0.7), 0.20, pan=-0.4 + 0.2 * i)
    put(fx, t0, zap(600 + 110 * i, 1.5, 0.08), 0.14, pan=-0.4 + 0.2 * i)
put(fx, 13.75, sparkle([72, 76, 79, 84, 88, 91, 96], 0.045), 0.18)           # outro wordmark
put(fx, 14.13, zap(380, 2.1, 0.17), 0.26)                                  # last hop

# ---------------- reverb, master ----------------
send = music * 0.5 + fx
ir_t = tt(1.1)
ir = rng.standard_normal((len(ir_t), 2)) * np.exp(-ir_t * 5.0)[:, None]
ir[: int(0.01 * SR)] = 0
size = 1 << int(np.ceil(np.log2(N + len(ir))))
wet = np.stack([np.fft.irfft(np.fft.rfft(send[:, c], size) * np.fft.rfft(ir[:, c], size), size)[:N] for c in range(2)], axis=1)
wet /= np.abs(wet).max() + 1e-9
mix = music + drums + fx + ticks + wet * 0.4 * np.abs(send).max()
mix /= np.abs(mix).max()
mix = np.tanh(mix * 1.7) / np.tanh(1.7)
i = np.arange(N)
mix = mix * (np.minimum(1, i / (0.02 * SR)) * np.minimum(1, (N - i) / (0.5 * SR)) ** 1.5)[:, None] * 0.89
with wave.open(sys.argv[1], "wb") as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR)
    w.writeframes((mix * 32767).astype("<i2").tobytes())
