"""Gera um WAV de teste (estéreo, 48 kHz, 24 s): "fala" (ruído em rajadas com envelope),
um trecho de música (acordes) e um golpe forte, para o CI conferir o Apura sem arquivos reais.
Só Python puro (sem numpy).

Uso: python scripts/gerar-audio-teste.py saida.wav
"""
import math
import random
import struct
import sys
import wave

SR = 48000
random.seed(20261020)


def amostras():
    n = 24 * SR
    estado = [0.0, 0.0]
    for i in range(n):
        t = i / SR
        if t < 12:  # "fala": ruído filtrado em sílabas de 120–300 ms, com pausas
            silaba = math.sin(math.pi * ((t * 4.3) % 1.0)) ** 2 if (int(t * 1.7) % 5) else 0.0
            for c in range(2):
                estado[c] = 0.97 * estado[c] + 0.03 * random.uniform(-1, 1)
            v = [0.9 * silaba * estado[0] * 6, 0.9 * silaba * estado[1] * 6]
        elif t < 22:  # música: acordes com um pouco de estéreo
            f = [220, 277.2, 329.6] if int(t) % 2 else [196, 246.9, 293.7]
            s = sum(math.sin(2 * math.pi * fi * t) for fi in f) / 3
            v = [0.25 * s + 0.02 * random.uniform(-1, 1), 0.22 * s + 0.02 * random.uniform(-1, 1)]
        else:  # golpe forte e silêncio
            v = [0.95 * math.sin(2 * math.pi * 80 * t) * (1 if t < 22.3 else 0)] * 2
        yield v


def main():
    with wave.open(sys.argv[1], "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(3)
        w.setframerate(SR)
        quadros = bytearray()
        for l, r in amostras():
            for v in (l, r):
                q = max(-8388608, min(8388607, int(v * 8388607)))
                quadros += struct.pack("<i", q)[:3]
        w.writeframes(bytes(quadros))


if __name__ == "__main__":
    main()
