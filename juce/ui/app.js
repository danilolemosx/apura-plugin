// Apura: janela do plugin. Cada controle é ligado a um parâmetro pelo JUCE (juce.js).
// Dois jeitos de funcionar, que a janela descobre sozinha:
//  - REAL TIME (insert, ex.: no master): espectro ao vivo, LISTEN aprende o volume;
//  - WHOLE CLIP (AudioSuite): o Preview/Render analisa o clip inteiro.
// Aberta num navegador comum (sem o plugin), mostra uma demonstração ao vivo.
// Script comum (não módulo): o WebKit do macOS não carrega módulos pelo endereço do plugin.

// faixas dos parâmetros (as mesmas de PluginProcessor.cpp); skew dos pontos: centro geométrico
const skewCentro = (de, ate, centro) => Math.log(0.5) / Math.log((centro - de) / (ate - de));
const PARAMS = {
  target:      { start: -31, end: -9, skew: 1, padrao: -23 },
  ceiling:     { start: -3, end: 0, skew: 1, padrao: -1 },
  highPass:    { start: 0, end: 120, skew: 1, padrao: 0 },
  width:       { start: -6, end: 6, skew: 1, padrao: 0 },
  gain:        { start: -30, end: 30, skew: 1, padrao: 0 },
  lowFreq:     { start: 20, end: 500, skew: skewCentro(20, 500, 100), padrao: 80 },
  lowMidFreq:  { start: 100, end: 2000, skew: skewCentro(100, 2000, 450), padrao: 300 },
  highMidFreq: { start: 1000, end: 10000, skew: skewCentro(1000, 10000, 3160), padrao: 2500 },
  highFreq:    { start: 3000, end: 20000, skew: skewCentro(3000, 20000, 7750), padrao: 10000 },
  lowGain:     { start: -12, end: 12, skew: 1, padrao: 0 },
  lowMidGain:  { start: -12, end: 12, skew: 1, padrao: 0 },
  highMidGain: { start: -12, end: 12, skew: 1, padrao: 0 },
  highGain:    { start: -12, end: 12, skew: 1, padrao: 0 },
};
const PONTOS = [
  { nome: "LOW", freq: "lowFreq", ganho: "lowGain", tipo: "shelfGrave" },
  { nome: "LOW-MID", freq: "lowMidFreq", ganho: "lowMidGain", tipo: "sino" },
  { nome: "HIGH-MID", freq: "highMidFreq", ganho: "highMidGain", tipo: "sino" },
  { nome: "HIGH", freq: "highFreq", ganho: "highGain", tipo: "shelfAgudo" },
];
const DESTINOS = [
  { nome: "MUSIC", alvo: -14, teto: -1, texto: "MUSIC · −14 LUFS · −1 dBTP", para: "music streaming" },
  { nome: "INTERNET", alvo: -16, teto: -1, texto: "INTERNET · −16 LUFS · −1 dBTP", para: "the internet" },
  { nome: "TV", alvo: -23, teto: -1, texto: "TV · EBU R128 · −23 LUFS · −1 dBTP", para: "TV" },
];
const N_CARATERES = 3;
const CARATER_MOTOR = [3, 0, 1]; // TRANSPARENT, DENSE (CLEAN), WARM (VALVE)
const BANDAS = [25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250, 1600, 2000,
  2500, 3150, 4000, 5000, 6300, 8000, 10000, 12500, 16000, 20000];

// −∞ (silêncio) chega do plugin como null: vira "—"
const valido = (v) => typeof v === "number" && isFinite(v);
// sem "−0.0": o que arredonda para zero vira zero
const limpo = (v, casas) => (Math.abs(v) < 0.5 * Math.pow(10, -casas) ? 0 : v);
const sinal = (v, casas = 1) => (valido(v) ? (limpo(v, casas) > 0 ? "+" : "") + limpo(v, casas).toFixed(casas).replace("-", "−") : "—");
const num = (v, casas = 1) => (valido(v) ? limpo(v, casas).toFixed(casas).replace("-", "−") : "—");
const hz = (f) => (f < 1000 ? Math.round(f) + " Hz" : (f / 1000).toFixed(f < 10000 ? 1 : 0) + " kHz");
const tempo = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
const FORMATO = {
  target: (v) => num(v) + " LUFS",
  ceiling: (v) => num(v) + " dBTP",
  gain: (v) => sinal(v) + " dB",
  highPass: (v) => (v < 30 ? "OFF" : Math.round(v) + " Hz"),
  width: (v) => (Math.abs(v) < 0.25 ? "0 dB" : sinal(v) + " dB"),
};

// ---------------------------------------------------------------------------------------------
// estado dos parâmetros: o do JUCE, ou um de demonstração

function ouvintes() {
  const l = new Set();
  return { addListener: (f) => l.add(f), callListeners: () => l.forEach((f) => f()) };
}
function faixa(nome) {
  const s = sliders[nome], p = s.properties;
  return p && p.end > p.start ? { start: p.start, end: p.end, skew: p.skew || 1 } : PARAMS[nome];
}
function sliderDemo(nome) {
  const p = PARAMS[nome];
  const s = {
    valor: p.padrao, valueChangedEvent: ouvintes(),
    getScaledValue: () => s.valor,
    getNormalisedValue: () => Math.pow((s.valor - p.start) / (p.end - p.start), p.skew),
    setNormalisedValue: (n) => {
      s.valor = Math.pow(Math.min(1, Math.max(0, n)), 1 / p.skew) * (p.end - p.start) + p.start;
      s.valueChangedEvent.callListeners();
    },
    sliderDragStarted: () => {}, sliderDragEnded: () => {},
  };
  return s;
}
function comboDemo(indice, n) {
  const c = {
    value: indice / (n - 1), properties: { choices: new Array(n).fill("") }, valueChangedEvent: ouvintes(),
    getChoiceIndex: () => Math.round(c.value * (n - 1)),
    setChoiceIndex: (i) => { c.value = i / (n - 1); c.valueChangedEvent.callListeners(); },
  };
  return c;
}
function chaveDemo(valor) {
  const c = { valor, valueChangedEvent: ouvintes(), getValue: () => c.valor, setValue: (v) => { c.valor = v; c.valueChangedEvent.callListeners(); } };
  return c;
}

const juce = window.__JUCE__ && window.Juce ? window.Juce : null;
const sliders = {};
for (const nome of Object.keys(PARAMS)) sliders[nome] = juce ? juce.getSliderState(nome) : sliderDemo(nome);
const combos = {
  destination: juce ? juce.getComboBoxState("destination") : comboDemo(2, DESTINOS.length),
  character: juce ? juce.getComboBoxState("character") : comboDemo(0, N_CARATERES),
};
const chaves = {
  listen: juce ? juce.getToggleState("listen") : chaveDemo(false),
  compare: juce ? juce.getToggleState("compare") : chaveDemo(false),
};
const nativo = (nome) => (juce ? juce.getNativeFunction(nome) : null);

const valor = (nome) => sliders[nome].getScaledValue();
const indice = (combo, n) => Math.min(n - 1, Math.max(0, Math.round(combo.value * (n - 1))));
function paraNormal(nome, v) {
  const f = faixa(nome);
  return Math.pow(Math.min(1, Math.max(0, (v - f.start) / (f.end - f.start))), f.skew);
}
function definir(nome, v) {
  const s = sliders[nome];
  s.sliderDragStarted();
  s.setNormalisedValue(paraNormal(nome, v));
  s.sliderDragEnded();
}
function escolher(combo, i, n) {
  if (!juce || combo.properties.choices.length === n) combo.setChoiceIndex(i);
  else {
    combo.value = i / (n - 1);
    window.__JUCE__.backend.emitEvent(combo.identifier, { eventType: "valueChanged", value: combo.value });
    combo.valueChangedEvent.callListeners();
  }
}

// ---------------------------------------------------------------------------------------------
// EQ: as mesmas fórmulas do motor (motor/src/eq4.rs)

const SR = 48000;
function coef(tipo, f0, g) {
  const w0 = (2 * Math.PI * Math.min(Math.max(f0, 10), 0.45 * SR)) / SR, s = Math.sin(w0), c = Math.cos(w0);
  const A = Math.pow(10, g / 40);
  let b, a;
  if (tipo === "sino") {
    const al = s / (2 * 0.9);
    b = [1 + al * A, -2 * c, 1 - al * A]; a = [1 + al / A, -2 * c, 1 - al / A];
  } else if (tipo === "passaAltas") {
    const al = s / (2 * Math.SQRT1_2);
    b = [(1 + c) / 2, -(1 + c), (1 + c) / 2]; a = [1 + al, -2 * c, 1 - al];
  } else {
    const al = s / (2 * Math.SQRT1_2), r = 2 * Math.sqrt(A) * al;
    if (tipo === "shelfGrave") {
      b = [A * (A + 1 - (A - 1) * c + r), 2 * A * (A - 1 - (A + 1) * c), A * (A + 1 - (A - 1) * c - r)];
      a = [A + 1 + (A - 1) * c + r, -2 * (A - 1 + (A + 1) * c), A + 1 + (A - 1) * c - r];
    } else {
      b = [A * (A + 1 + (A - 1) * c + r), -2 * A * (A - 1 + (A + 1) * c), A * (A + 1 + (A - 1) * c - r)];
      a = [A + 1 - (A - 1) * c + r, 2 * (A - 1 - (A + 1) * c), A + 1 - (A - 1) * c - r];
    }
  }
  return [b[0] / a[0], b[1] / a[0], b[2] / a[0], a[1] / a[0], a[2] / a[0]];
}
function respostaDb(k, f) {
  const w = (2 * Math.PI * f) / SR, c1 = Math.cos(w), s1 = Math.sin(w), c2 = Math.cos(2 * w), s2 = Math.sin(2 * w);
  const nr = k[0] + k[1] * c1 + k[2] * c2, ni = -(k[1] * s1 + k[2] * s2);
  const dr = 1 + k[3] * c1 + k[4] * c2, di = -(k[3] * s1 + k[4] * s2);
  return 10 * Math.log10((nr * nr + ni * ni) / (dr * dr + di * di));
}
function eqAtual() {
  const ks = PONTOS.filter((p) => Math.abs(valor(p.ganho)) > 1e-6).map((p) => coef(p.tipo, valor(p.freq), valor(p.ganho)));
  const hp = valor("highPass");
  if (hp >= 30) ks.push(coef("passaAltas", hp, 0));
  return (f) => ks.reduce((soma, k) => soma + respostaDb(k, f), 0);
}

// ---------------------------------------------------------------------------------------------
// estado vindo do plugin

let medida = null;      // AudioSuite: último clip
let vivo = null;        // insert: medidas ao vivo
let aprendizado = null; // insert: último LISTEN
let estado = { audiosuite: false, processando: false, latenciaMs: 0 };
let referenciaVivo = -200;
const modoClip = () => !!(juce && estado.audiosuite);
const $ = (id) => document.getElementById(id);

function ajustesMudaram() {
  if (!medida || !medida.analisado || !medida.ajustes) return false;
  const a = medida.ajustes, perto = (x, y) => Math.abs(x - y) < 0.05;
  const hp = valor("highPass");
  if (!perto(a.alvo, valor("target")) || !perto(a.teto, valor("ceiling")) || !perto(a.hp, hp >= 30 ? hp : 0) ||
      !perto(a.width, valor("width")) || a.carater !== CARATER_MOTOR[indice(combos.character, N_CARATERES)] ||
      a.comparar !== (chaves.compare.getValue() ? 1 : 0)) return true;
  return PONTOS.some((p, k) => !perto(a.freq[k], valor(p.freq)) || !perto(a.ganho[k], valor(p.ganho)));
}

// ---------------------------------------------------------------------------------------------
// gráfico: espectro e os 4 pontos

const svg = $("espectro");
let selecionado = 1, arrastando = -1;
const geometria = () => {
  const W = svg.clientWidth || 440, H = svg.clientHeight || 200;
  const X = (f) => (Math.log(f / 20) / Math.log(1000)) * W;
  const F = (x) => 20 * Math.pow(1000, x / W);
  const meio = H * 0.46, escala = (H * 0.4) / 12;
  return { W, H, X, F, meio, Ye: (d) => meio - d * escala, D: (y) => (meio - y) / escala };
};

function desenharGrafico() {
  const { W, H, X, F, meio, Ye } = geometria();
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
  let g = "";
  for (const f of [50, 200, 500, 2000, 5000]) g += `<line x1="${X(f)}" y1="0" x2="${X(f)}" y2="${H}" stroke="#2c2a28"/>`;
  for (const f of [100, 1000, 10000]) g += `<line x1="${X(f)}" y1="0" x2="${X(f)}" y2="${H}" stroke="#383633"/>`;
  g += `<line x1="0" y1="${meio}" x2="${W}" y2="${meio}" stroke="#4b4844" stroke-dasharray="3 3"/>`;

  const eq = eqAtual();
  let entrada = null, saida = null, prevista = false;
  if (modoClip() && medida && medida.analisado) {
    entrada = medida.espIn;
    if (chaves.compare.getValue()) saida = entrada;
    else if (ajustesMudaram()) { saida = entrada.map((d, i) => (valido(d) ? d + eq(BANDAS[i]) : d)); prevista = true; }
    else saida = medida.espOut.map((d) => (valido(d) ? d - (medida.lufsOut - medida.lufsIn) : d));
  } else if (!modoClip() && vivo) {
    entrada = vivo.espIn;
    saida = vivo.espOut.map((d) => (valido(d) && valido(vivo.ganhoDb) ? d - vivo.ganhoDb : d));
  }
  if (entrada) {
    // referência que acompanha o nível devagar (o desenho não pula)
    const topo = Math.max(...entrada.filter(valido), -200);
    referenciaVivo = topo > referenciaVivo ? topo : referenciaVivo - 0.15;
    const ref = modoClip() ? topo : referenciaVivo;
    const Ys = (d) => 8 + ((ref + 3 - (valido(d) ? d : -200)) / 54) * (H - 20);
    const caminho = (v) => v.map((d, i) => `${i ? "L" : "M"}${X(BANDAS[i]).toFixed(1)} ${Math.min(H, Ys(d)).toFixed(1)}`).join(" ");
    g += `<path d="${caminho(entrada)} L${W} ${H} L0 ${H} Z" fill="#3a3835"/>`;
    if (saida) g += `<path d="${caminho(saida)}" fill="none" stroke="#efe5cf" stroke-width="1.5" opacity="0.9" ${prevista ? 'stroke-dasharray="5 3"' : ""}/>`;
  }
  // a curva da EQ e os pontos
  let c = "";
  for (let x = 0; x <= W; x += 3) c += `${x ? "L" : "M"}${x} ${Math.max(2, Math.min(H - 2, Ye(eq(F(x))))).toFixed(1)}`;
  g += `<path d="${c}" fill="none" stroke="#d2382f" stroke-width="2.3"/>`;
  PONTOS.forEach((p, k) => {
    const sel = k === selecionado;
    g += `<circle data-k="${k}" cx="${X(valor(p.freq)).toFixed(1)}" cy="${Ye(valor(p.ganho)).toFixed(1)}" r="${sel ? 8 : 6.5}"
      fill="${sel ? "#d2382f" : "#34322f"}" stroke="#efe5cf" stroke-width="2" style="cursor:grab"/>`;
  });
  for (const [f, t] of [[100, "100"], [1000, "1K"], [10000, "10K"]])
    g += `<text x="${X(f) + 3}" y="${H - 5}" fill="#6f695f" font-size="9" font-weight="700" font-family="Archivo">${t}</text>`;
  g += `<text x="${W - 4}" y="${meio - 4}" fill="#6f695f" font-size="9" text-anchor="end" font-family="Archivo">0 dB</text>`;
  svg.innerHTML = g;

  const p = PONTOS[selecionado];
  $("infoEq").textContent = `${p.nome} ${sinal(valor(p.ganho))} dB · ${hz(valor(p.freq))}`;
  const vazio = $("vazio");
  if (modoClip() && !(medida && medida.analisado)) { vazio.textContent = "Preview or render a clip to see it here"; vazio.style.display = "flex"; }
  else if (!modoClip() && juce && !(vivo && valido(vivo.momentaneo))) { vazio.textContent = "Play the sequence to see it here"; vazio.style.display = "flex"; }
  else vazio.style.display = "none";
}

svg.addEventListener("pointerdown", (e) => {
  const c = e.target.closest("circle");
  if (!c) return;
  arrastando = selecionado = +c.dataset.k;
  svg.setPointerCapture(e.pointerId);
  const p = PONTOS[arrastando];
  sliders[p.freq].sliderDragStarted();
  sliders[p.ganho].sliderDragStarted();
  desenharGrafico();
});
svg.addEventListener("pointermove", (e) => {
  if (arrastando < 0) return;
  const { F, D } = geometria(), r = svg.getBoundingClientRect(), p = PONTOS[arrastando];
  const f = faixa(p.freq);
  const freq = Math.min(f.end, Math.max(f.start, F(e.clientX - r.left)));
  const ganho = Math.round(Math.min(12, Math.max(-12, D(e.clientY - r.top))) * 10) / 10;
  sliders[p.freq].setNormalisedValue(paraNormal(p.freq, freq));
  sliders[p.ganho].setNormalisedValue(paraNormal(p.ganho, ganho));
});
const soltar = () => {
  if (arrastando < 0) return;
  const p = PONTOS[arrastando];
  sliders[p.freq].sliderDragEnded();
  sliders[p.ganho].sliderDragEnded();
  arrastando = -1;
};
svg.addEventListener("pointerup", soltar);
svg.addEventListener("pointercancel", soltar);
svg.addEventListener("dblclick", (e) => {
  const c = e.target.closest("circle");
  if (c) definir(PONTOS[+c.dataset.k].ganho, 0);
});
for (const p of PONTOS) for (const n of [p.freq, p.ganho]) sliders[n].valueChangedEvent.addListener(() => agendar());

$("match").addEventListener("click", async () => {
  $("avisoEq").textContent = "";
  let ok = false;
  if (juce) ok = await nativo("match")();
  else {
    // demonstração: uma sugestão típica de som direto
    [[100, -1.5], [350, -2.5], [3150, 1.5], [10000, -0.5]].forEach(([f, g], k) => { definir(PONTOS[k].freq, f); definir(PONTOS[k].ganho, g); });
    ok = true;
  }
  if (!ok) $("avisoEq").textContent = modoClip() ? "Preview the clip first" : "Use Listen first";
});
$("flat").addEventListener("click", () => PONTOS.forEach((p) => definir(p.ganho, 0)));

// ---------------------------------------------------------------------------------------------
// volume

function desenharVolume() {
  const alvo = valor("target");
  let agulha = null, palavra = "—", classe = "";
  if (modoClip()) {
    if (medida && medida.analisado) agulha = medida.lufsOut;
  } else if (vivo && valido(vivo.curto)) agulha = vivo.curto;
  if (valido(agulha)) {
    const d = agulha - alvo, tol = modoClip() ? 0.6 : 2;
    if (Math.abs(d) < tol) { palavra = "ON TARGET"; classe = "ok"; }
    else if (d < 0) { palavra = "TOO QUIET"; classe = "baixo"; }
    else { palavra = "TOO LOUD"; classe = "alto"; }
  } else if (!modoClip() && juce) palavra = "NO SIGNAL";
  const el = $("palavra");
  el.textContent = palavra;
  el.className = "palavra " + classe;

  const de = alvo - 20, ate = alvo + 10, px = (v) => 10 + ((Math.min(ate, Math.max(de, v)) - de) / (ate - de)) * 200;
  let v = `<rect x="10" y="22" width="200" height="10" rx="2" fill="#141312"/>`;
  v += `<rect x="${px(alvo - 1)}" y="20" width="${px(alvo + 1) - px(alvo - 1)}" height="14" rx="2" fill="#8fd9a8" opacity="0.35"/>`;
  if (valido(agulha)) v += `<line x1="${px(agulha)}" y1="14" x2="${px(agulha)}" y2="40" stroke="#efe5cf" stroke-width="3" stroke-linecap="round"/>`;
  v += `<text x="10" y="54" fill="#6f695f" font-size="9" font-family="Archivo" font-weight="700">QUIET</text>`;
  v += `<text x="${px(alvo)}" y="54" fill="#8fd9a8" font-size="9" font-family="Archivo" font-weight="800" text-anchor="middle">TARGET</text>`;
  v += `<text x="210" y="54" fill="#6f695f" font-size="9" font-family="Archivo" font-weight="700" text-anchor="end">LOUD</text>`;
  $("medidor").innerHTML = v;

  const destino = DESTINOS[indice(combos.destination, DESTINOS.length)];
  const btn = $("listen"), info = $("infoVolume");
  btn.classList.toggle("oculto", modoClip());
  if (modoClip()) {
    if (estado.processando) info.textContent = "Analyzing the clip…";
    else if (medida && medida.analisado)
      info.textContent = `Clip: ${num(medida.lufsIn)} → ${num(medida.lufsOut)} LUFS · gain ${sinal(medida.ganho)} dB` +
        (medida.noTeto ? ". Gain is capped at +15 dB: the clip is very quiet." : "");
    else info.textContent = `Preview or render the clip. Apura measures it whole and sets it for ${destino.para}.`;
  } else {
    const escutando = !!chaves.listen.getValue();
    btn.classList.toggle("escutando", escutando);
    if (escutando) {
      btn.textContent = "■ LISTENING " + tempo(vivo && valido(vivo.escutandoS) ? vivo.escutandoS : 0);
      info.textContent = "Play the sequence. Press again when it has heard enough.";
    } else if (aprendizado && aprendizado.valido) {
      btn.textContent = "● LISTEN AGAIN";
      const noTeto = indice(combos.destination, DESTINOS.length) !== 0 && valido(aprendizado.ganhoSugerido) && aprendizado.ganhoSugerido >= 14.99;
      info.textContent = `Learned ${tempo(aprendizado.segundos)}. Volume set ${sinal(ganhoEfetivo())} dB for ${destino.para}.` +
        (noTeto ? " That is the maximum: this sequence is very quiet." : "");
    } else {
      btn.textContent = "● LISTEN";
      info.textContent = `Press Listen and play the sequence. Apura learns its volume and sets it for ${destino.para}.`;
    }
  }
  const lim = modoClip() ? (medida && medida.lim) : (vivo && vivo.lim);
  const forte = valido(lim) && lim < -6;
  $("protecao").textContent = forte ? "ON · WORKING HARD" : "ON";
  $("protecao").classList.toggle("trabalhando", forte);
}
$("listen").addEventListener("click", () => chaves.listen.setValue(!chaves.listen.getValue()));
// o ganho aplicado de fato: GAIN + o quanto o alvo mudou desde o LISTEN (PluginProcessor.cpp)
const ganhoEfetivo = () => valor("gain") + (aprendizado && aprendizado.valido && valido(aprendizado.alvo) ? valor("target") - aprendizado.alvo : 0);
chaves.listen.valueChangedEvent.addListener(() => agendar());

// ---------------------------------------------------------------------------------------------
// destino, caráter, comparação e o painel avançado

const teclasDestino = [...document.querySelectorAll(".destino")];
const teclasCarater = [...document.querySelectorAll("#carater .tecla")];
teclasDestino.forEach((b) => b.addEventListener("click", () => {
  const i = +b.dataset.i;
  if (juce) nativo("destino")(i);
  else { escolher(combos.destination, i, DESTINOS.length); definir("target", DESTINOS[i].alvo); definir("ceiling", DESTINOS[i].teto); }
}));
teclasCarater.forEach((b) => b.addEventListener("click", () => escolher(combos.character, +b.dataset.i, N_CARATERES)));
combos.destination.valueChangedEvent.addListener(() => agendar());
combos.character.valueChangedEvent.addListener(() => agendar());
$("compare").addEventListener("click", () => chaves.compare.setValue(!chaves.compare.getValue()));
chaves.compare.valueChangedEvent.addListener(() => agendar());
$("atsc").addEventListener("click", () => {
  if (juce) nativo("destino")(2); else escolher(combos.destination, 2, DESTINOS.length);
  setTimeout(() => { definir("target", -24); definir("ceiling", -2); }, juce ? 120 : 0);
});
$("abrirAvancado").addEventListener("click", () => { $("avancado").classList.remove("oculto"); agendar(); });
$("fecharAvancado").addEventListener("click", () => $("avancado").classList.add("oculto"));

// knobs do painel avançado: 270°, arrastar para cima/baixo, duplo clique = padrão
function ponto(r, graus) {
  const a = ((graus - 90) * Math.PI) / 180;
  return [50 + r * Math.cos(a), 50 + r * Math.sin(a)];
}
function arco(r, de, ate) {
  const [x1, y1] = ponto(r, de), [x2, y2] = ponto(r, ate);
  return `M${x1.toFixed(2)} ${y1.toFixed(2)} A${r} ${r} 0 ${ate - de > 180 ? 1 : 0} 1 ${x2.toFixed(2)} ${y2.toFixed(2)}`;
}
const knobs = [...document.querySelectorAll(".knob")];
function desenharKnob(el) {
  const nome = el.dataset.param, s = sliders[nome];
  const v = Math.min(1, Math.max(0, s.getNormalisedValue()));
  const ang = -135 + 270 * v, de = el.dataset.bipolar ? 0 : -135;
  const [a0, a1] = ang >= de ? [de, ang] : [ang, de];
  const [px, py] = ponto(19, ang);
  el.querySelector("svg").innerHTML =
    `<path d="${arco(40, -135, 135)}" fill="none" stroke="#3a3835" stroke-width="6" stroke-linecap="round"/>` +
    (a1 - a0 > 0.5 ? `<path d="${arco(40, a0, a1)}" fill="none" stroke="${el.dataset.cor}" stroke-width="6" stroke-linecap="round"/>` : "") +
    `<circle cx="50" cy="50" r="28" fill="#34322f" stroke="#4a4742" stroke-width="1.5"/>` +
    `<line x1="50" y1="50" x2="${px.toFixed(2)}" y2="${py.toFixed(2)}" stroke="#efe5cf" stroke-width="4" stroke-linecap="round"/>`;
  el.querySelector(".v").textContent = FORMATO[nome](s.getScaledValue());
}
for (const el of knobs) {
  const nome = el.dataset.param, s = sliders[nome], k = el.querySelector("svg");
  k.setAttribute("viewBox", "0 0 100 100");
  k.setAttribute("width", el.dataset.tam);
  k.setAttribute("height", el.dataset.tam);
  s.valueChangedEvent.addListener(() => { desenharKnob(el); agendar(); });
  k.addEventListener("pointerdown", (e) => {
    k.setPointerCapture(e.pointerId);
    const y0 = e.clientY, n0 = s.getNormalisedValue();
    s.sliderDragStarted();
    const mover = (ev) => s.setNormalisedValue(n0 + ((y0 - ev.clientY) / 200) * (ev.shiftKey ? 0.2 : 1));
    const fim = () => { k.removeEventListener("pointermove", mover); k.removeEventListener("pointerup", fim); s.sliderDragEnded(); };
    k.addEventListener("pointermove", mover);
    k.addEventListener("pointerup", fim);
  });
  k.addEventListener("dblclick", () => definir(nome, PARAMS[nome].padrao));
}

function desenharNumeros() {
  const linhas = [];
  if (modoClip() && medida && medida.analisado) {
    linhas.push(["IN", num(medida.lufsIn) + " LUFS"], ["OUT", num(medida.lufsOut) + " LUFS"], ["GAIN", sinal(medida.ganho) + " dB"],
      ["TRUE PEAK", `${num(medida.tpIn)} → ${num(medida.tpOut)}`], ["LRA", `${num(medida.lraIn)} → ${num(medida.lraOut)} LU`],
      ["LIMITER", valido(medida.lim) && medida.lim < -0.05 ? num(medida.lim) + " dB" : "—"]);
  } else if (vivo) {
    linhas.push(["MOMENTARY", num(vivo.momentaneo) + " LUFS"], ["SHORT-TERM", num(vivo.curto) + " LUFS"], ["INTEGRATED", num(vivo.integrado) + " LUFS"],
      ["PEAK", num(vivo.pico) + " dBFS"], ["LIMITER", valido(vivo.lim) && vivo.lim < -0.05 ? num(vivo.lim) + " dB" : "—"],
      ["COMPRESSOR", valido(vivo.comp) && vivo.comp < -0.05 ? num(vivo.comp) + " dB" : "—"]);
  }
  $("numeros").innerHTML = linhas.map(([a, b]) => `<dt>${a}</dt><dd class="tab">${b}</dd>`).join("") || "<dt>NO MEASUREMENT YET</dt><dd></dd>";
  $("latencia").textContent = !modoClip() && estado.latenciaMs > 0 ? `LATENCY ${num(estado.latenciaMs, 1)} MS` : "";
}

// ---------------------------------------------------------------------------------------------
// tudo junto

function atualizar() {
  const d = indice(combos.destination, DESTINOS.length), car = indice(combos.character, N_CARATERES);
  teclasDestino.forEach((b, i) => b.classList.toggle("ligado", i === d));
  teclasCarater.forEach((b, i) => b.classList.toggle("ligada", i === car));
  $("compare").classList.toggle("ligada", !!chaves.compare.getValue());

  const alvo = valor("target"), teto = valor("ceiling"), dest = DESTINOS[d];
  const padrao = Math.abs(alvo - dest.alvo) < 0.01 && Math.abs(teto - dest.teto) < 0.01;
  const atsc = Math.abs(alvo + 24) < 0.01 && Math.abs(teto + 2) < 0.01;
  $("padrao").textContent = padrao ? dest.texto : atsc ? "TV · ATSC A/85 · −24 LKFS · −2 dBTP" : `CUSTOM · ${num(alvo)} LUFS · ${num(teto)} dBTP`;

  const selo = $("selo"), status = $("status");
  status.classList.remove("aviso");
  if (!juce && vitrine) { selo.className = "selo azul"; selo.textContent = "REAL TIME"; status.textContent = vivo && valido(vivo.integrado) ? `Integrated since Listen: ${num(vivo.integrado)} LUFS · peak −1.0 dBFS` : ""; }
  else if (!juce) { selo.className = "selo"; selo.textContent = "DEMO"; status.textContent = "Demo: drag the points, press Listen, change the destination."; }
  else if (modoClip()) {
    if (estado.processando) { selo.className = "selo aviso"; selo.textContent = "ANALYZING…"; }
    else if (medida && medida.erro) { selo.className = "selo erro"; selo.textContent = medida.canais > 2 ? "MONO OR STEREO ONLY" : "ERROR " + medida.erro; }
    else if (medida && medida.analisado) { selo.className = "selo ok"; selo.textContent = "WHOLE CLIP · ANALYZED"; }
    else { selo.className = "selo"; selo.textContent = "WHOLE CLIP"; }
    if (medida && medida.analisado && ajustesMudaram()) { status.textContent = "Settings changed — preview again to update the numbers."; status.classList.add("aviso"); }
    else if (medida && medida.analisado)
      status.textContent = `${tempo(medida.segundos)} · ${num(medida.taxa / 1000, 1)} kHz · ${medida.canais === 1 ? "mono" : "stereo"} · ${num(medida.tempo)} s to process`;
    else status.textContent = "";
  } else {
    const escutando = !!chaves.listen.getValue();
    selo.className = escutando ? "selo aviso" : "selo azul";
    selo.textContent = escutando ? "LISTENING…" : "REAL TIME";
    status.textContent = vivo && valido(vivo.integrado) ? `Integrated since Listen: ${num(vivo.integrado)} LUFS · peak ${num(vivo.pico)} dBFS` : "";
  }
  desenharGrafico();
  desenharVolume();
  if (!$("avancado").classList.contains("oculto")) desenharNumeros();
}

// redesenha no próximo quadro (vários eventos juntos viram um desenho só)
let pedido = false;
function agendar() {
  if (pedido) return;
  pedido = true;
  requestAnimationFrame(() => { pedido = false; atualizar(); });
}

if (window.__JUCE__) {
  const b = window.__JUCE__.backend;
  b.addEventListener("medida", (d) => { medida = d; agendar(); });
  b.addEventListener("aoVivo", (d) => { vivo = { ...d, ganhoDb: ganhoEfetivo() }; agendar(); });
  b.addEventListener("aprendizado", (d) => { aprendizado = d; agendar(); });
  b.addEventListener("estado", (d) => { estado = d; agendar(); });
} else {
  // demonstração ao vivo: um som direto que oscila, com LISTEN funcionando
  let t = 0, escuta = 0;
  const base = BANDAS.map((f) => { const l = Math.log2(f / 400); return -40 - l * l * 1.6 + (f < 90 ? -(90 / f) * 4 : 0); });
  setInterval(() => {
    t += 1 / 15;
    const nivelIn = -28.5 + Math.sin(t * 0.9) * 2.5 + Math.sin(t * 2.3) * 1.2;
    const eq = eqAtual();
    const espIn = base.map((v, i) => v + nivelIn + 28.5 + Math.sin(i * 0.7 + t * 3) * 1.8);
    const g = ganhoEfetivo();
    const espOut = espIn.map((v, i) => v + eq(BANDAS[i]) + g);
    if (chaves.listen.getValue()) escuta += 1 / 15;
    vivo = { momentaneo: nivelIn + g, curto: nivelIn + g + Math.sin(t * 0.4), integrado: aprendizado ? valor("target") : null,
      pico: -6 + g * 0.3, lim: g > 4 ? -1.2 : 0, comp: 0, espIn, espOut, escutandoS: escuta, ganhoDb: g };
    agendar();
  }, 1000 / 15);
  chaves.listen.valueChangedEvent.addListener(() => {
    if (chaves.listen.getValue()) escuta = 0;
    else if (escuta > 0) { aprendizado = { valido: true, segundos: escuta, lufsEntrada: -28.5, alvo: valor("target") }; definir("gain", valor("target") + 28.5); }
  });
}
// vitrine (index.html#vitrine, para a imagem do README): MATCH aplicado e volume já aprendido
const vitrine = !juce && location.hash === "#vitrine";
if (vitrine) {
  [[100, -1.5], [350, -2.5], [3150, 1.5], [10000, -0.5]].forEach(([f, g], k) => { definir(PONTOS[k].freq, f); definir(PONTOS[k].ganho, g); });
  aprendizado = { valido: true, segundos: 387, lufsEntrada: -28.5, alvo: -23 };
  definir("gain", 5.5);
}
new ResizeObserver(() => agendar()).observe(svg);
knobs.forEach(desenharKnob);
atualizar();

// conferência para os testes (lida pelo index.html)
window.__apuraResumo = () => ({
  juce: !!juce,
  modo: modoClip() ? "clip" : "vivo",
  selo: $("selo").textContent,
  destino: indice(combos.destination, DESTINOS.length),
  pontos: svg.querySelectorAll("circle").length,
  knobs: knobs.map((k) => k.querySelector(".v").textContent),
  fonte: document.fonts.check("12px Archivo"),
});
