"""Gera o kit de sinais de teste para medir o Mastering Assistant do Logic Pro.

Uso: python referencia/gerar_sinais.py <pasta de saída>

Cada arquivo é um experimento separado: o Mastering Assistant analisa o arquivo
inteiro, então misturar testes num arquivo só contaminaria a análise.
Saída: WAV 48 kHz / 24 bit / estéreo + manifesto.json com os parâmetros.
"""
import json
import sys
from pathlib import Path

import numpy as np
import pyloudnorm as pyln
import soundfile as sf

SR = 48000
# pasta de saída: o primeiro argumento (padrão: sinais-de-teste/ na pasta atual)
SAIDA = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("sinais-de-teste")
SAIDA.mkdir(parents=True, exist_ok=True)
rng = np.random.default_rng(20261020)


def db(x):
    return 10 ** (x / 20)


def ruido_colorido(n, forma_db=None):
    """Ruído rosa (1/f) com uma curva extra opcional forma_db(f) em dB."""
    espectro = np.fft.rfft(rng.standard_normal(n))
    f = np.fft.rfftfreq(n, 1 / SR)
    f[0] = f[1]
    ganho = 1 / np.sqrt(f)
    if forma_db is not None:
        ganho *= db(forma_db(f))
    ganho[f < 15] = 0  # sem subsônico
    x = np.fft.irfft(espectro * ganho, n)
    x /= np.std(x)
    return np.clip(x, -4, 4)  # fator de crista ~12 dB, perto de uma mix real


def estereo(mid, side):
    return np.stack([mid + side, mid - side], axis=1)


def rms_db(x):
    return 20 * np.log10(np.sqrt(np.mean(x ** 2)) + 1e-20)


def ajustar_rms(x, alvo_db):
    return x * db(alvo_db - rms_db(x))


def fade(x, ms=50):
    k = int(SR * ms / 1000)
    rampa = np.linspace(0, 1, k)[:, None]
    x[:k] *= rampa
    x[-k:] *= rampa[::-1]
    return x


def ruido_estereo(seg, forma_db=None, side_db=-10.0):
    n = int(seg * SR)
    mid = ruido_colorido(n, forma_db)
    side = ruido_colorido(n, forma_db)
    side = side / np.std(side) * np.std(mid) * db(side_db)
    return estereo(mid, side)


# Curvas de forma espectral (dB em função de f)
def inclinacao(db_por_oitava):
    return lambda f: db_por_oitava * np.log2(f / 1000)


def shelf_grave(ganho, fc):
    return lambda f: ganho / (1 + (f / fc) ** 4)


def sino(ganho, fc, oitavas=1.0):
    return lambda f: ganho * np.exp(-0.5 * (np.log2(f / fc) / (oitavas / 2.355)) ** 2)


sinais = {}


def registrar(nome, x, descricao, **params):
    x = fade(x.copy())
    pico = np.max(np.abs(x))
    if pico > db(-0.5):
        raise ValueError(f"{nome}: pico {20*np.log10(pico):.1f} dBFS")
    caminho = SAIDA / f"{nome}.wav"
    sf.write(caminho, x, SR, subtype="PCM_24")
    sinais[nome] = {
        "arquivo": caminho.name,
        "descricao": descricao,
        "duracao_s": round(len(x) / SR, 3),
        "rms_dbfs": round(rms_db(x), 2),
        "pico_dbfs": round(20 * np.log10(pico), 2),
        "lufs_integrado": round(pyln.Meter(SR).integrated_loudness(x), 2),
        **params,
    }
    print(f"{nome:22s} {sinais[nome]['lufs_integrado']:7.2f} LUFS  pico {sinais[nome]['pico_dbfs']:6.2f}")


# --- 1. Ruído rosa em três volumes: alvo de loudness e ganho -----------------
for nome, nivel in [("01_ROSA_REF", -20), ("02_ROSA_BAIXO", -32), ("03_ROSA_QUENTE", -14)]:
    registrar(nome, ajustar_rms(ruido_estereo(30), nivel),
              "Ruído rosa estéreo (side -10 dB)", rms_alvo=nivel)

# --- 2. Espectros desequilibrados: como o Auto EQ corrige ---------------------
formas = [
    ("04_ESCURO", inclinacao(-3), "Rosa inclinado -3 dB/oitava (abafado)", {"inclinacao_db_oit": -3}),
    ("05_BRILHANTE", inclinacao(3), "Rosa inclinado +3 dB/oitava (brilhante)", {"inclinacao_db_oit": 3}),
    ("06_GRAVE_DEMAIS", shelf_grave(8, 120), "Rosa +8 dB abaixo de 120 Hz", {"shelf_db": 8, "shelf_hz": 120}),
    ("07_MEDIO_EMBOLADO", sino(6, 400, 1.0), "Rosa +6 dB em 400 Hz (1 oitava)", {"sino_db": 6, "sino_hz": 400}),
    ("08_SEM_GRAVE", shelf_grave(-12, 150), "Rosa -12 dB abaixo de 150 Hz (magro)", {"shelf_db": -12, "shelf_hz": 150}),
    ("09_PRESENCA_DURA", sino(6, 3500, 1.0), "Rosa +6 dB em 3,5 kHz (agressivo)", {"sino_db": 6, "sino_hz": 3500}),
]
for nome, forma, desc, params in formas:
    registrar(nome, ajustar_rms(ruido_estereo(30, forma), -20), desc, rms_alvo=-20, **params)

# --- 3. Degraus de volume: curva estática do compressor/limitador ------------
niveis = list(range(-44, -13, 6))  # -44 ... -14
sequencia = niveis + niveis[-2::-1]
blocos = [ajustar_rms(ruido_estereo(3), n) for n in sequencia]
registrar("10_DEGRAUS", np.concatenate(blocos),
          "Rosa em degraus de 6 dB, 3 s cada, sobe e desce",
          degraus_rms=sequencia, degrau_s=3)

# --- 4. Rajadas: ataque e release --------------------------------------------
base = ajustar_rms(ruido_estereo(30), -32)
envelope = np.ones(len(base))
rajadas = []
for i, t in enumerate(np.arange(2.0, 28.0, 2.5)):
    dur = 0.4 if i % 2 == 0 else 0.05
    a, b = int(t * SR), int((t + dur) * SR)
    envelope[a:b] = db(18)  # -32 -> -14 dB RMS
    rajadas.append({"inicio_s": round(float(t), 3), "duracao_s": dur})
registrar("11_RAJADAS", base * envelope[:, None],
          "Rosa a -32 dB RMS com rajadas de +18 dB (400 ms e 50 ms)",
          rms_base=-32, rajada_db=18, rajadas=rajadas)

# --- 5. Senoides sobre leito de ruído: saturação de cada caráter --------------
partes, eventos, t0 = [], [], 0.0
for freq in (100, 1000):
    for pico in (-30, -18, -12, -6, -3):
        n = 2 * SR
        tom = db(pico) * np.sin(2 * np.pi * freq * np.arange(n) / SR)
        partes.append(np.stack([tom, tom], axis=1))
        eventos.append({"inicio_s": t0, "duracao_s": 2.0, "freq_hz": freq, "pico_dbfs": pico})
        t0 += 2.0
tons = np.concatenate(partes)
leito = ajustar_rms(ruido_estereo(len(tons) / SR), -40)
x = tons + leito
x *= min(1.0, db(-0.6) / np.max(np.abs(x)))
registrar("12_SENOIDES", x, "Senoides 100 Hz e 1 kHz em 5 volumes sobre rosa a -40 dB",
          eventos=eventos, leito_rms=-40)

# --- 6. Imagem estéreo: largura e grave mono ----------------------------------
n = 6 * SR
segmentos = []
def bloco(mid, side):
    return ajustar_rms(estereo(mid, side), -20)
m = ruido_colorido(n); segmentos.append(("mono", bloco(m, 0 * m)))
m, s = ruido_colorido(n), ruido_colorido(n); segmentos.append(("normal_side_-10", bloco(m, s * 0.316)))
l, r = ruido_colorido(n), ruido_colorido(n); segmentos.append(("descorrelacionado", ajustar_rms(np.stack([l, r], 1), -20)))
m, s = ruido_colorido(n), ruido_colorido(n); segmentos.append(("side_igual_mid", bloco(m, s)))
grave_side = ruido_colorido(n, lambda f: np.where(f < 150, 0, -60))
agudo_mid = ruido_colorido(n, lambda f: np.where(f < 150, -60, 0))
segmentos.append(("grave_so_no_side", bloco(agudo_mid, grave_side * 2)))
registrar("13_ESTEREO", np.concatenate([s for _, s in segmentos]),
          "Segmentos de 6 s: mono, normal, descorrelacionado, side=mid, grave só no side",
          segmentos=[{"nome": nome, "inicio_s": 6.0 * i, "duracao_s": 6.0} for i, (nome, _) in enumerate(segmentos)])

(SAIDA / "manifesto.json").write_text(
    json.dumps({"taxa_hz": SR, "bits": 24, "sinais": sinais}, ensure_ascii=False, indent=2))
print(f"\n{len(sinais)} arquivos em {SAIDA}")
