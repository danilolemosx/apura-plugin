# Especificação: plugin de master inspirado no Mastering Assistant

Versão 1.2, 20/10/2026 (corrigida com o protótipo; modo vídeo na seção 6). Feita só por **medição de comportamento**: 13 sinais de
teste passados pelo Mastering Assistant do Logic Pro (31 bounces, 17 prints).
Nenhum código ou binário da Apple foi aberto. Todos os números vêm de
`02_RENDERS` comparados com `01_SINAIS_TESTE`.

Como ler: **medido** = número tirado dos bounces; **inferido** = a explicação
mais simples que bate com as medidas; **a confirmar** = ainda falta teste.

---

## 1. Visão geral

O plugin funciona em dois tempos, igual ao do Logic:

1. **Análise** (offline, uma vez): lê a música inteira e decide quatro
   números/curvas: a curva de EQ, o ganho de loudness, a largura do estéreo e
   o que mostrar no painel.
2. **Processamento** (tempo real): aplica essas decisões numa cadeia fixa. O
   **caráter** escolhe só o estágio de dinâmica; ele **não muda** a EQ nem a
   largura (medido: curvas iguais nos 4 caracteres, diferença ≤ 0,5 dB).

```
entrada → Auto EQ → Width (M/S) → ganho → [compressor, só no Clean] → [saturação, só no Valve] → limitador −1 dBTP → saída
```

---

## 2. Análise

### 2.1 Espectro de entrada
- Média de longo prazo do espectro (soma L+R), suavizada em **1/3 de oitava**.
- Desvio `D(f)` = espectro medido − ruído rosa (inclinação −3 dB/oitava),
  com o nível médio removido (o loudness cuida do nível).

### 2.2 Curva de EQ (Auto EQ)
**Medido** em ruído rosa plano (a "curva-alvo" `T(f)`, dB relativos a 1 kHz):

| Hz | 30 | 60 | 100 | 200 | 400 | 600 | 1k | 2k | 3,5k | 5k | 7k | 10k | 15k |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| T | −0,2 | +1,1 | +1,2 | +0,7 | −0,4 | −0,6 | 0 | −0,3 | −1,3 | −2,1 | −3,1 | −2,9 | −0,8 |

**Inferido** (bate com os testes 04 a 09 e 12):

```
EQ(f) = limita( T(f) − k · D(f) , −4 dB , +3,5 dB )
k ≈ 0,4 (cortes de picos estreitos) a 0,3 (desvios largos)
```

- Picos de +5 a +6 dB numa banda viram cortes de **−2 a −2,2 dB**
  (07: 400 Hz; 09: 3,15 kHz).
- Inclinações enormes (±20 dB de ponta a ponta) param no limite:
  no máximo uns **+4,5 / −3,8 dB** de correção (04 e 05).
- **Não mexe abaixo de ~30 Hz nem acima de ~12 kHz**: a curva volta a 0 dB
  nas duas pontas, em qualquer teste.
- Ressonância estreita (tom de 1 kHz dominando, teste 12) gera um **corte
  estreito de ~−3 dB**. A resolução da curva é fina, não só 3 ou 4 filtros.
- O volume de entrada não muda a curva (01, 02 e 03 iguais).
- **A confirmar:** se os boosts são mais limitados que os cortes (no 05 o
  corte de agudo parou em −2,2 dB extra; no 08 o boost de grave parou em
  +1,5 dB extra). Para o plugin, começar simétrico e ajustar com música real.

Implementação sugerida: EQ de fase mínima com curva desenhada (FIR de fase
mínima ou ~24 bandas de 1/3 de oitava em biquads), mais um controle "Auto EQ
%" que escala `EQ(f)` de 0 a 100% (o Logic tem esse controle).

### 2.3 Loudness (ganho)
- **Medido:** saída integrada em **−14 LUFS** no Clean, qualquer que seja a
  entrada (−28,4, −16,4 ou −10,4 LUFS → −14,07 nos três).
- O ganho é calculado **depois** da EQ e da dinâmica: num arquivo dinâmico o
  limitador come os trechos fortes, e o ganho sobe para compensar (no
  `10_DEGRAUS`, o Clean levantou os trechos baixos em **+8,4 dB**).
- Nos outros caracteres a saída fica entre **−13,6 e −15,4 LUFS**. Eles
  soltam mais dinâmica e não "corrigem" o ganho para bater exato.
- **Inferido:** ganho = (−14 − LUFS medido depois de EQ e dinâmica), com uma
  ou duas iterações da análise para convergir.

### 2.4 Largura (Width)
**Medido** (razão Side/Mid, dB):

| Entrada | Saída | Mudança |
|---|---|---|
| −39,6 (quase mono) | −33,9 | +5,7 |
| −10,0 (normal) | −8,9 a −9,4 | +0,6 a +1,4 |
| −2,2 (muito aberto) | −6,5 | −4,3 |

**Inferido:**

```
ganho_side_dB = limita( 0,7 · (−8,4 − S/M_entrada) , −6 , +6 )
```

- É **um ganho fixo no Side** para a música inteira, igual em todas as
  frequências e nos 4 caracteres.
- **Não há grave em mono**: o grave no side é reduzido como o resto.
- Mono continua mono (não inventa estéreo).

---

## 3. Processamento por caráter

### 3.1 Tabela resumo (medido)

| | Clean | Valve | Punch | Transparent |
|---|---|---|---|---|
| Latência total | 48 amostras (1 ms) | 24 (0,5 ms) | 24 | 24 |
| Atraso extra no grave (100 Hz) | nenhum | ~65 amostras (1,3 ms) | ~65 | ~60 |
| Ganho em sinal fraco (`10_DEGRAUS`) | **+8,4 dB** | +4,4 | +3,7 | +4,4 |
| Saída no degrau mais forte (−14 dB RMS) | −12,8 | −12,6 | **−11,4** | **−11,4** |
| Redução numa rajada de +18 dB | **−9 dB** | −5,5 a −6 | −4 a −5 | −4,5 a −5,5 |
| Redução extra no grave (<150 Hz) | 0 | +0,7 dB | +1,2 dB | +1,4 dB |
| Ataque (63% da redução) | ~2 ms | ~2–5 ms | ~2–5 ms | ~2–5 ms |
| Release (volta a −1 dB) | ~150–200 ms | ~150–200 ms | ~150–200 ms | ~150–200 ms |
| Compressor antes do limitador (protótipo) | **sim**, 2:1 | não | não | não |
| Harmônicos com tom a −3 dBFS | ~−65 dB | H3 ~−57 dB | ~−65 dB | ~−65 dB |

### 3.2 Clean (corrigido na v1.1, depois do protótipo)
- **Compressor de banda larga + limitador.** É o único caráter com
  compressor: ele levanta os trechos baixos (+8,4 dB) bem mais que os altos
  (+1,1 dB). A v1 desta especificação dizia o contrário; o protótipo mostrou
  o erro.
- Valores ajustados contra os bounces (testes 10 e 11 e o trecho real de som direto):
  limiar −16,5 dB (RMS de 5 ms, escala de pico), razão **1,8:1**, joelho
  6 dB, ataque **30 ms**, release **150 ms**. A razão é meio-termo: os
  degraus de ruído pedem 2:1, a fala do som direto pede ~1,6:1. Janelas de medição
  mais longas (50–400 ms) não resolveram esse conflito.
- Limitador: lookahead 1 ms, release 50 ms, teto **−1 dBTP** (todos os
  bounces com material forte saíram com pico −1,04 dBFS).
- Resultado prático: é o caráter que **mais achata** a dinâmica.

### 3.3 Valve, Punch, Transparent
- **Ganho fixo + limitador**, sem compressor: o ganho é igual (+3,6 a
  +4,5 dB) do degrau mais baixo até −20 dB, e só o limitador age no topo.
- Limitador ajustado: release **50 ms no Valve** e **~2 ms no Punch e no
  Transparent** (quase um clipper suavizado). É isso que deixa o trecho mais
  forte sair ~1,2 dB mais alto nesses dois.
- Um pouco mais de redução no grave (0,7 a 1,4 dB) e o grave atrasado
  ~1,3 ms: indício de **limitador em bandas** (crossover perto de 150 Hz).
  O protótipo ainda usa banda única.
- **Valve**: mais uma **saturação muito leve de harmônicos ímpares** (H3
  ~8 dB acima dos outros, ainda ~0,15%). No protótipo: `tanh` suave.
- **Punch × Transparent**: no ruído de teste são quase iguais.
  **A confirmar** com música real.
- O loudness desses três no Logic varia ±0,8 LUFS em torno de −14 de
  arquivo para arquivo; a regra exata não apareceu nos testes.

### 3.4 Excite
Não foi testado (ficou desligado em todos os bounces). Fica de fora da
versão 1, ou vira um gerador de harmônicos de agudo próprio.

---

## 4. O que ainda é incerto

1. **Release dos caracteres não-Clean.** A medida ficou ~150–200 ms, mas o
   rabo do grave atrapalha. Um teste com rajadas só de médio-agudo resolve.
2. **Saturação fina.** O ruído de fundo do `12_SENOIDES` esconde distorção
   abaixo de ~−60 dB. Um tom limpo, sem ruído, mede melhor.
3. **Frequência e inclinação do crossover** dos caracteres não-Clean.
   Um teste com degraus de volume só no grave e só no agudo separa as bandas.
4. **Comportamento com música real.** Ruído é estacionário; música tem
   transientes e silêncios. Duas ou três mixes sem master
   validam tudo.

---

## 5. Validação do protótipo (v1.1)

`scripts/prototipo.py` processou os 13 sinais nos 4 caracteres
(`05_PROTOTIPO`), e `scripts/comparar.py` mediu tudo contra o Logic
(relatório completo em `05_PROTOTIPO/relatorio.txt`):

| Medida | Protótipo × Logic |
|---|---|
| Auto EQ, por banda | erro médio **0,19 dB**, máximo 1,0 dB |
| Loudness integrado | erro médio +0,2 LUFS, máximo 1,4 (caracteres não-Clean) |
| Width (S/M) | ≤ 0,8 dB, exceto no 12 (quase mono) |
| Degraus, Clean | ≤ 0,2 dB em todos os degraus |
| Degraus, outros | 0,4 a 1,2 dB abaixo (diferença de loudness) |
| Redução nas rajadas | Clean −11,2 × −9,1; Valve −7,0 × −5,9; Punch/Transparent −7,7 × −4,5/−5,1 |

**Material real** (bounces do Logic do som direto e do trailer, Clean e Punch):

| Trecho | | Saída LUFS | Ganho típico | Variação do ganho |
|---|---|---|---|---|
| Som direto, Clean | Logic / protótipo | −13,85 / −14,09 | +21,2 / +22,9 | 10,1 / 14,0 dB |
| Som direto, Punch | Logic / protótipo | −14,37 / −14,02 | +18,2 / +18,2 | 6,3 / 5,8 dB |
| Trailer, Clean | Logic / protótipo | −13,97 / −14,04 | +5,2 / +5,3 | 8,5 / 9,9 dB |
| Trailer, Punch | Logic / protótipo | −13,37 / −13,96 | +2,5 / +1,2 | 3,4 / 1,6 dB |

O Logic **também** esmaga som direto: +21 dB de ganho e ~10 dB de
compressão no som direto, e ainda reforça 40–90 Hz na fala. Não há proteção para
diálogo no Mastering Assistant.

**O que falta acertar:** o protótipo segura os golpes 1 a 3 dB a mais que o
Logic, e o Clean ainda varia o ganho ~40% a mais que o do Logic na fala. A suspeita é o limitador em bandas (item 3.3); com uma banda só, um
golpe de grave abaixa tudo.

---

## 6. Modo vídeo (desenho nosso, não copiado)

O Mastering Assistant é feito para música (−14 LUFS). Para vídeo o plugin
ganha um seletor de **entrega**, que muda o alvo e a análise; o caráter
continua escolhendo só a dinâmica.

| Modo | Alvo | Teto | EQ | Width | Teto de ganho |
|---|---|---|---|---|---|
| Música | −14 LUFS | −1 dBTP | curva do Logic (seção 2.2) | automática | sem |
| Web | −16 LUFS | −1 dBTP | voz | intocada | +15 dB |
| TV (EBU R128) | −23 LUFS | −1 dBTP | voz | intocada | +15 dB |
| TV EUA (ATSC A/85) | −24 LKFS | −2 dBTP | voz | intocada | +15 dB |

**EQ de voz:**
- passa-altas fixo de 2ª ordem em **60 Hz** (−12 dB/oitava). Tira ronco,
  vento e manuseio sem afinar a música de um trailer; 80 Hz tirava −3,8 dB
  em 100 Hz no trailer;
- o desvio é medido contra o espectro médio da fala (LTASS, Byrne et al.
  1994), não contra ruído rosa. Assim a voz não é tratada como "médio
  embolado";
- correção suave: 30% do desvio, entre −3 e +2 dB, só de 100 Hz a 10 kHz;
- **nunca reforça abaixo de 150 Hz.**

**Dinâmica:** o limiar do compressor (Clean) acompanha o alvo (−16,5 dB
vale para −14 LUFS; em TV vira −25,5 dB). O teto de ganho de +15 dB evita
levar um som direto muito baixo a "explodir": se o alvo não couber, o plugin
avisa em vez de esmagar.

**Testes com os 4 trechos** (`scripts/testar_video.py`, saídas em
`05_PROTOTIPO/VIDEO`):
- **Punch/Transparent** chegam no alvo com compressão quase nula (mediana
  −0,1 dB; pior 5% entre −0,7 e −6,5 dB) e preservam a dinâmica (LRA
  13–17 LU). São os caracteres naturais para vídeo.
- **Clean** fica denso (LRA cai 2–6 LU) e às vezes bate no teto de ganho
  (som direto e vídeo mono em Web saem em −19 LUFS). Serve como opção "denso, para
  celular e redes".
- Todos os casos de TV saíram em −23,0 ± 0,05 LUFS com TP ≤ −1 dBTP.
- **Valve, Punch e Transparent saem quase iguais** no protótipo, porque as
  medidas com ruído não separaram Punch de Transparent. Precisa de mais
  material real.

**Ideias para a v2 do modo vídeo:** loudness medido só na fala (gate de
diálogo, como pede a Netflix), controle de faixa de loudness (LRA alvo) e
detecção de mono (a vinheta e o vídeo mono são L = R).

---

## 7. Plano de construção

1. **Protótipo em Python** (offline): implementar análise + cadeia acima e
   processar os 13 sinais de teste.
2. **Validação**: comparar protótipo × Logic com as mesmas medidas (LUFS, EQ
   por banda, curva de degraus, redução nas rajadas, S/M). Meta: diferenças
   ≤ 0,5 dB na EQ e ≤ 1 dB na dinâmica. Ajustar `k`, limites e constantes de
   tempo até bater.
3. **Validação de ouvido** com as suas mixes: Logic × protótipo, lado a lado,
   no mesmo volume.
4. **Plugin AU** no mesmo esquema do `Decanta.component`, com a análise num
   botão "Analisar" e os controles: Caráter, Auto EQ %, Loudness, Width,
   Bypass com compensação de volume.

Scripts de apoio: `scripts/conferir.py` (confere bounces) e `scripts/eq.py`
(mede a EQ aplicada).
