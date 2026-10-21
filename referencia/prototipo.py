"""Protótipo offline do master, seguindo ESPECIFICACAO.md.

Uso:
  .venv/bin/python scripts/prototipo.py ARQUIVO.wav CARATER [--modo musica|web|tv|tv_eua] [--saida X.wav]
  .venv/bin/python scripts/prototipo.py --kit        # processa os 13 sinais nos 4 caracteres

O modo "musica" imita o Mastering Assistant (medido). Os modos de vídeo são
desenho nosso: alvo de loudness de entrega, EQ pensada para voz, largura
intocada e teto de ganho.
"""
import argparse
from pathlib import Path

import numpy as np
import pyloudnorm as pyln
import soundfile as sf
from numba import njit
from scipy.signal import fftconvolve, resample_poly, welch

RAIZ = Path(__file__).resolve().parent.parent
CARATERES = ["CLEAN", "VALVE", "PUNCH", "TRANSPARENT"]

# ---------------------------------------------------------------- parâmetros
TETO_DB = -1.0

# Bandas de 1/3 de oitava e a curva-alvo T medida no ruído rosa plano (01)
BANDAS = np.array([25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800,
                   1000, 1250, 1600, 2000, 2500, 3150, 4000, 5000, 6300, 8000, 10000, 12500, 16000, 20000])
ALVO_T = np.interp(np.log(BANDAS), np.log([25, 30, 60, 100, 200, 400, 600, 1000, 2000, 3500, 5000, 7000, 10000, 15000, 20000]),
                   [0.0, -0.2, 1.1, 1.2, 0.7, -0.4, -0.6, 0.0, -0.3, -1.3, -2.1, -3.1, -2.9, -0.8, 0.0])
EQ = dict(k=0.4, minimo=-4.5, maximo=2.6)

# Curva de referência para fala: aproximação do espectro médio de longo prazo da
# voz (LTASS, Byrne et al. 1994), em potência por banda de 1/3 de oitava, dB
# relativos a 1 kHz. No modo vídeo o desvio é medido contra ela, não contra o rosa.
REF_VOZ = np.interp(np.log(BANDAS), np.log([25, 63, 80, 100, 125, 200, 250, 400, 500, 800, 1000, 2000, 4000, 8000, 10000, 20000]),
                    [-25, -12, -6, -3, 1, 6, 7, 6, 5, 1, 0, -5, -8, -11, -13, -20])
EQ_VOZ = dict(k=0.3, minimo=-3.0, maximo=2.0, passa_altas_hz=60.0, sem_reforco_abaixo_hz=150.0)

# Modos de entrega. limiar do compressor acompanha o alvo (−16,5 dB a −14 LUFS).
MODOS = {
    "musica": dict(alvo_lufs=-14.0, teto_db=-1.0, eq="musica", width=True, ganho_max_db=None),
    "web": dict(alvo_lufs=-16.0, teto_db=-1.0, eq="voz", width=False, ganho_max_db=15.0),
    "tv": dict(alvo_lufs=-23.0, teto_db=-1.0, eq="voz", width=False, ganho_max_db=15.0),      # EBU R128
    "tv_eua": dict(alvo_lufs=-24.0, teto_db=-2.0, eq="voz", width=False, ganho_max_db=15.0),  # ATSC A/85
}

# Largura: ganho no Side = limita(k · (alvo − S/M), ±limite)
WIDTH = dict(alvo=-8.4, k=0.7, limite=6.0)

# Dinâmica por caráter (medido: o Clean comprime antes do limitador; os outros
# três são ganho fixo + limitador). comp=None → só limitador.
DINAMICA = {
    "CLEAN": dict(comp=dict(limiar=-16.5, razao=1.8, joelho=6.0), lim_release_ms=50.0, satur=0.0),
    "VALVE": dict(comp=None, lim_release_ms=50.0, satur=0.15),
    "PUNCH": dict(comp=None, lim_release_ms=2.0, satur=0.0),
    "TRANSPARENT": dict(comp=None, lim_release_ms=2.0, satur=0.0),
}
# Ajustados por busca contra os bounces do Logic (testes 10 e 11 e um som direto), 20/10/2026.
# Razão 1,8 é meio-termo: os degraus pedem 2,0 e a fala do som direto pede ~1,6.
TEMPOS = dict(ataque_ms=30.0, release_ms=150.0, rms_ms=5.0, lookahead_ms=1.0)


# ---------------------------------------------------------------- análise
def espectro_bandas(x, sr):
    f, p = welch(x.sum(1), sr, nperseg=16384)
    db = np.empty(len(BANDAS))
    for i, fc in enumerate(BANDAS):
        sel = (f >= fc / 2 ** (1 / 6)) & (f < fc * 2 ** (1 / 6))
        db[i] = 10 * np.log10(p[sel].sum() + 1e-20)  # potência por banda: rosa = plano
    return db


def peso_pontas(f):
    """1 entre 50 Hz e 8 kHz, cai a 0 em 25 Hz e 16 kHz (medido: EQ nunca mexe nas pontas)."""
    lf = np.clip(np.log2(f / 25) / np.log2(50 / 25), 0, 1)
    hf = np.clip(np.log2(16000 / f) / np.log2(16000 / 8000), 0, 1)
    return lf * hf


def curva_voz(db):
    """EQ do modo vídeo: passa-altas fixo + correção suave contra a referência de voz."""
    uteis = (BANDAS >= 100) & (BANDAS <= 10000)
    desvio = db - REF_VOZ
    desvio -= desvio[uteis].mean()
    peso = np.clip(np.log2(BANDAS / 63) / np.log2(100 / 63), 0, 1) * np.clip(np.log2(16000 / BANDAS), 0, 1)
    correcao = np.clip(-EQ_VOZ["k"] * desvio * peso, EQ_VOZ["minimo"], EQ_VOZ["maximo"])
    correcao[BANDAS < EQ_VOZ["sem_reforco_abaixo_hz"]] = np.minimum(correcao[BANDAS < EQ_VOZ["sem_reforco_abaixo_hz"]], 0)
    passa_altas = 10 * np.log10(1 / (1 + (EQ_VOZ["passa_altas_hz"] / BANDAS) ** 4))  # 2ª ordem, −12 dB/oitava
    return correcao + passa_altas


def analisar(x, sr, modo="musica"):
    db = espectro_bandas(x, sr)
    m, s = x[:, 0] + x[:, 1], x[:, 0] - x[:, 1]
    sm = 10 * np.log10(np.mean(s ** 2) / np.mean(m ** 2) + 1e-20)
    if MODOS[modo]["eq"] == "voz":
        curva = curva_voz(db)
    else:
        uteis = (BANDAS >= 40) & (BANDAS <= 12500)
        desvio = db - db[uteis].mean()
        # a curva-alvo vale inteira; só a correção some nas pontas
        curva = np.clip(ALVO_T - EQ["k"] * desvio * peso_pontas(BANDAS), EQ["minimo"], EQ["maximo"])
    side_db = float(np.clip(WIDTH["k"] * (WIDTH["alvo"] - sm), -WIDTH["limite"], WIDTH["limite"])) \
        if MODOS[modo]["width"] else 0.0
    return dict(eq_db=curva, side_db=side_db, sm_entrada=sm, modo=modo)


# ---------------------------------------------------------------- blocos
def aplicar_eq(x, sr, eq_db, n=8192):
    """FIR de fase linear desenhado na curva (offline; o plugin usará fase mínima)."""
    f = np.fft.rfftfreq(n, 1 / sr)
    fq = np.maximum(f, 1.0)
    ganho = 10 ** (np.interp(np.log(fq), np.log(BANDAS), eq_db) / 20)
    h = np.fft.irfft(ganho, n)
    h = np.roll(h, n // 2) * np.hanning(n)
    y = np.stack([fftconvolve(x[:, c], h)[n // 2: n // 2 + len(x)] for c in range(2)], 1)
    return y


def aplicar_width(x, side_db):
    m, s = (x[:, 0] + x[:, 1]) / 2, (x[:, 0] - x[:, 1]) / 2 * 10 ** (side_db / 20)
    return np.stack([m + s, m - s], 1)


@njit(cache=True)
def _compressor_ganho(nivel_db, limiar, razao, joelho, a_at, a_rel):
    g = np.empty_like(nivel_db)
    estado = 0.0
    for i in range(len(nivel_db)):
        acima = nivel_db[i] - limiar
        if 2 * acima < -joelho:
            alvo = 0.0
        elif 2 * acima > joelho:
            alvo = acima * (1 / razao - 1)
        else:
            alvo = (1 / razao - 1) * (acima + joelho / 2) ** 2 / (2 * joelho)
        coef = a_at if alvo < estado else a_rel
        estado = coef * estado + (1 - coef) * alvo
        g[i] = estado
    return g


def compressor(x, sr, limiar, razao, joelho):
    rms = TEMPOS["rms_ms"] * sr / 1000
    p = _media_movel((x ** 2).mean(1), int(rms))
    nivel = 10 * np.log10(p + 1e-20) + 3.0  # +3: RMS de ruído estéreo em escala de pico de senoide
    a_at = np.exp(-1 / (TEMPOS["ataque_ms"] * sr / 1000))
    a_rel = np.exp(-1 / (TEMPOS["release_ms"] * sr / 1000))
    g = _compressor_ganho(nivel, limiar, razao, joelho, a_at, a_rel)
    return x * 10 ** (g[:, None] / 20)


def _media_movel(v, n):
    c = np.cumsum(np.concatenate([[0.0], v]))
    out = np.empty_like(v)
    out[n:] = (c[n + 1:] - c[1:-n]) / n
    out[:n] = c[1:n + 1] / np.arange(1, n + 1)
    return out


@njit(cache=True)
def _release(g, a_rel):
    out = np.empty_like(g)
    estado = 1.0
    for i in range(len(g)):
        if g[i] < estado:
            estado = g[i]
        else:
            estado = a_rel * estado + (1 - a_rel) * g[i]
        out[i] = estado
    return out


def limitador(x, sr, teto_db=TETO_DB, release_ms=170.0):
    teto = 10 ** (teto_db / 20)
    la = int(TEMPOS["lookahead_ms"] * sr / 1000)
    # pico verdadeiro aproximado: detecção com 4x oversampling
    pico = np.abs(resample_poly(x, 4, 1, axis=0)).max(1).reshape(-1, 4).max(1)[: len(x)]
    req = np.minimum(1.0, teto / np.maximum(pico, 1e-12))
    # mínimo no horizonte de lookahead, depois release, depois suaviza o ataque
    req = np.lib.stride_tricks.sliding_window_view(np.concatenate([req, np.ones(la)]), la + 1).min(1)
    g = _release(req, np.exp(-1 / (release_ms * sr / 1000)))
    g = np.minimum(g, _media_movel(np.concatenate([g, np.ones(la)]), la + 1)[la:])
    return x * g[:, None]


def dinamica(x, sr, carater, modo="musica"):
    cfg, m = DINAMICA[carater], MODOS[modo]
    if cfg["comp"] is not None:
        c = cfg["comp"]
        limiar = c["limiar"] + (m["alvo_lufs"] - MODOS["musica"]["alvo_lufs"])
        x = compressor(x, sr, limiar, c["razao"], c["joelho"])
    if cfg["satur"] > 0:
        d = cfg["satur"]
        x = np.tanh(d * 3 * x) / (d * 3)  # saturação suave, ganho unitário em sinal baixo
    return limitador(x, sr, teto_db=m["teto_db"], release_ms=cfg["lim_release_ms"])


# ---------------------------------------------------------------- cadeia
def masterizar(x, sr, carater, analise=None, max_iter=8, modo=None):
    a = analise or analisar(x, sr, modo or "musica")
    modo = a.get("modo", "musica")
    m = MODOS[modo]
    pre = aplicar_width(aplicar_eq(x, sr, a["eq_db"]), a["side_db"])
    medidor = pyln.Meter(sr)
    ganho = m["alvo_lufs"] - medidor.integrated_loudness(pre)
    teto_ganho = m["ganho_max_db"] if m["ganho_max_db"] is not None else np.inf
    # o ganho mexe na compressão/limitação, então repete até a saída cair no alvo
    # (o Logic erra até ±0,8 LUFS nos caracteres não-Clean; não deu para modelar)
    for _ in range(max_iter):
        ganho = min(ganho, teto_ganho)
        y = dinamica(pre * 10 ** (ganho / 20), sr, carater, modo)
        aplicado = ganho  # o ganho que gerou este y
        erro = m["alvo_lufs"] - medidor.integrated_loudness(y)
        if abs(erro) < 0.05 or (ganho >= teto_ganho and erro > 0):
            break
        ganho += erro * 1.5
    a["ganho_db"] = aplicado
    a["no_teto_de_ganho"] = bool(aplicado >= teto_ganho)
    return y, a


def processar_kit():
    saida = RAIZ / "05_PROTOTIPO"
    saida.mkdir(exist_ok=True)
    for p in sorted((RAIZ / "01_SINAIS_TESTE").glob("*.wav")):
        x, sr = sf.read(p)
        a = analisar(x, sr)
        for c in CARATERES:
            y, a = masterizar(x, sr, c, a)
            sf.write(saida / f"{p.stem}__{c}.wav", y, sr, subtype="PCM_24")
        print(f"{p.stem:20s} ganho {a['ganho_db']:+5.1f} dB  side {a['side_db']:+4.1f} dB")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("arquivo", nargs="?")
    ap.add_argument("--kit", action="store_true", help="processa os 13 sinais de teste nos 4 caracteres")
    ap.add_argument("carater", nargs="?", default="PUNCH")
    ap.add_argument("--modo", default="musica", choices=list(MODOS))
    ap.add_argument("--saida")
    args = ap.parse_args()
    if args.kit:
        processar_kit()
    else:
        x, sr = sf.read(args.arquivo, always_2d=True)
        y, a = masterizar(x, sr, args.carater.upper(), modo=args.modo)
        destino = args.saida or Path(args.arquivo).with_suffix(f".{args.modo}.{args.carater.upper()}.wav")
        sf.write(destino, y, sr, subtype="PCM_24")
        aviso = "  (no teto de ganho: alvo não alcançado)" if a["no_teto_de_ganho"] else ""
        print(f"ganho {a['ganho_db']:+.1f} dB, side {a['side_db']:+.1f} dB → {destino}{aviso}")
