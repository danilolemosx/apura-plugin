"""Gera os casos de referência que o motor em Rust tem que reproduzir.

Cada caso é um trecho curto passado pelo protótipo Python (prototipo.py):
  motor/tests/ref/<caso>/entrada.wav   trecho de entrada (float 32)
  motor/tests/ref/<caso>/saida.wav     resultado do protótipo (float 32)
  motor/tests/ref/<caso>/dados.json    análise, ganho e loudness

Uso (Python com numpy, scipy, soundfile, pyloudnorm e numba):
  python referencia/gerar_sinais.py <pasta dos sinais>          # os 13 sinais de teste
  python referencia/gerar_referencias.py <pasta dos sinais> [trechos.json]

`trechos.json` (opcional, fora do git) aponta para gravações reais, que entram como casos
a mais: {"som_direto": "/caminho/x.wav", "trailer": "/caminho/y.wav", "vinheta": "/caminho/z.wav"}
"""
import json
import sys
from pathlib import Path

import numpy as np
import pyloudnorm as pyln
import soundfile as sf

AQUI = Path(__file__).resolve().parent
sys.path.insert(0, str(AQUI))
import prototipo as P  # noqa: E402

SINAIS = Path(sys.argv[1])
TRECHOS = json.loads(Path(sys.argv[2]).read_text()) if len(sys.argv) > 2 else {}
DESTINO = AQUI.parent / "motor" / "tests" / "ref"

# (caso, arquivo, início s, duração s, modo, caracteres)
CASOS = [
    ("rosa", "sinal:01_ROSA_REF.wav", 0, 10, "musica", ["CLEAN", "VALVE", "PUNCH", "TRANSPARENT"]),
    ("degraus", "sinal:10_DEGRAUS.wav", 0, 33, "musica", ["CLEAN", "PUNCH"]),
    ("rajadas", "sinal:11_RAJADAS.wav", 0, 15, "musica", ["CLEAN", "VALVE"]),
    ("senoides", "sinal:12_SENOIDES.wav", 0, 10, "musica", ["VALVE"]),
    ("estereo", "sinal:13_ESTEREO.wav", 0, 30, "musica", ["CLEAN"]),
    ("som_direto", "trecho:som_direto", 60, 30, "tv", ["PUNCH", "CLEAN"]),
    ("som_direto_web", "trecho:som_direto", 60, 30, "web", ["CLEAN"]),
    ("trailer", "trecho:trailer", 0, 30, "musica", ["PUNCH"]),
    ("trailer_eua", "trecho:trailer", 0, 30, "tv_eua", ["TRANSPARENT"]),
    ("vinheta_441", "trecho:vinheta", 0, 20, "web", ["PUNCH"]),
]


def main():
    for caso, origem, ini, dur, modo, caracteres in CASOS:
        tipo, nome = origem.split(":", 1)
        if tipo == "trecho" and nome not in TRECHOS:
            continue  # gravação real não informada: o caso fica de fora
        arq = SINAIS / nome if tipo == "sinal" else Path(TRECHOS[nome])
        x, sr = sf.read(arq, always_2d=True)
        x = x[int(ini * sr): int((ini + dur) * sr)].astype(np.float32).astype(np.float64)
        medidor = pyln.Meter(sr)
        for c in caracteres:
            pasta = DESTINO / f"{caso}_{c.lower()}"
            pasta.mkdir(parents=True, exist_ok=True)
            sf.write(pasta / "entrada.wav", x, sr, subtype="FLOAT")
            a = P.analisar(x, sr, modo)
            y, a = P.masterizar(x, sr, c, dict(a))
            sf.write(pasta / "saida.wav", y, sr, subtype="FLOAT")
            dados = dict(
                sr=sr, modo=modo, carater=c,
                eq_db=[float(v) for v in a["eq_db"]],
                side_db=float(a["side_db"]), sm_entrada=float(a["sm_entrada"]),
                ganho_db=float(a["ganho_db"]), no_teto_de_ganho=a["no_teto_de_ganho"],
                lufs_entrada=float(medidor.integrated_loudness(x)),
                lufs_saida=float(medidor.integrated_loudness(y)),
            )
            (pasta / "dados.json").write_text(json.dumps(dados, indent=1))
            print(f"{pasta.name:22s} ganho {dados['ganho_db']:+6.2f}  saída {dados['lufs_saida']:6.2f} LUFS", flush=True)


if __name__ == "__main__":
    main()
