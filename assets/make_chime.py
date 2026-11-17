"""Gera assets/timer.wav: o "plim" do fim do timer.

Três notas suaves subindo (mi, sol#, si: um acorde de mi maior), timbre de sino de vidro
(senoide + harmônicos fracos), ataque macio e um eco curto. Toca duas vezes.
"""
import math
import struct
import wave

RATE = 44100


def bell(freq, dur, amp):
    out = []
    for i in range(int(RATE * dur)):
        t = i / RATE
        attack = min(1.0, t / 0.012)  # sem "clique" no começo
        env = attack * math.exp(-t * 4.2)
        v = (math.sin(2 * math.pi * freq * t)
             + 0.28 * math.sin(2 * math.pi * freq * 2.01 * t) * math.exp(-t * 7)
             + 0.10 * math.sin(2 * math.pi * freq * 3.98 * t) * math.exp(-t * 11))
        out.append(amp * env * v)
    return out


def mix(buf, part, start):
    s = int(start * RATE)
    if len(buf) < s + len(part):
        buf.extend([0.0] * (s + len(part) - len(buf)))
    for i, v in enumerate(part):
        buf[s + i] += v


notes = [(659.25, 0.0), (830.61, 0.13), (987.77, 0.26)]  # E5, G#5, B5
buf = []
for rep in range(2):
    base = rep * 1.05
    for f, at in notes:
        mix(buf, bell(f, 1.1, 0.22), base + at)
# Eco curto e baixo: dá "ar" sem virar reverb.
echo = [v * 0.18 for v in buf]
mix(buf, echo, 0.19)
peak = max(abs(v) for v in buf) or 1
buf = [v / peak * 0.6 for v in buf]
# Fim suave.
fade = int(RATE * 0.3)
for i in range(fade):
    buf[-fade + i] *= 1 - i / fade

with wave.open("assets/timer.wav", "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(RATE)
    w.writeframes(b"".join(struct.pack("<h", int(v * 32767)) for v in buf))
print(len(buf) / RATE, "s")
